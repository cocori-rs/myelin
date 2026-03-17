//! Rate-limiting middleware example.
//!
//! Demonstrates how to:
//! - Use interior mutability (`tokio::sync::Mutex`) for shared mutable state
//! - Reject requests that exceed a per-second threshold
//! - Pass the middleware trait's `&self` requirement while mutating state
//!
//! Run: `cargo run --example rate_limit`

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use async_trait::async_trait;
use http::{Request, Response};
use http_body::Body;
use tokio::sync::Mutex;
use tonic::Status;
use tower_service::Service;

use myelin::prelude::*;

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

struct RateLimitMiddleware {
    max_per_second: u32,
    state: Mutex<RateLimitState>,
}

struct RateLimitState {
    count: u32,
    window_start: Instant,
}

impl RateLimitMiddleware {
    fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            state: Mutex::new(RateLimitState {
                count: 0,
                window_start: Instant::now(),
            }),
        }
    }
}

#[derive(Default)]
struct EmptyBody;

impl Body for EmptyBody {
    type Data = bytes::Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }
}

#[async_trait]
impl GrpcMiddleware for RateLimitMiddleware {
    type Body = EmptyBody;

    async fn on_request(
        &self,
        req: Request<EmptyBody>,
        _ctx: &mut GrpcContext,
    ) -> Result<Request<EmptyBody>, Status> {
        let mut state = self.state.lock().await;

        // Reset window if a second has elapsed.
        if state.window_start.elapsed().as_secs() >= 1 {
            state.count = 0;
            state.window_start = Instant::now();
        }

        state.count += 1;

        if state.count > self.max_per_second {
            println!("  [rate_limit] REJECTED (count={})", state.count);
            return Err(Status::resource_exhausted("rate limit exceeded"));
        }

        println!("  [rate_limit] ALLOWED  (count={})", state.count);
        Ok(req)
    }

    async fn on_response(
        &self,
        res: Response<EmptyBody>,
        _ctx: &GrpcContext,
    ) -> Result<Response<EmptyBody>, Status> {
        Ok(res)
    }
}

// ---------------------------------------------------------------------------
// Fake inner service
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct EchoService;

impl Service<Request<EmptyBody>> for EchoService {
    type Response = Response<EmptyBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request<EmptyBody>) -> Self::Future {
        Box::pin(async {
            Ok(Response::builder()
                .status(200)
                .header("grpc-status", "0")
                .body(EmptyBody)
                .unwrap())
        })
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Allow only 3 requests per second.
    let layer = MyelinLayer::new(RateLimitMiddleware::new(3));
    let mut svc = tower_layer::Layer::layer(&layer, EchoService);

    println!("Sending 5 requests (limit: 3/sec):\n");

    for i in 1..=5 {
        let req = Request::builder().body(EmptyBody).unwrap();
        let res = svc.call(req).await.unwrap();
        let status = res.headers().get("grpc-status").unwrap().to_str().unwrap();
        let msg = res
            .headers()
            .get("grpc-message")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("ok");
        println!("  Request {}: grpc-status={} ({})\n", i, status, msg);
    }
}
