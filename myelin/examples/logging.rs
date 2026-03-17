//! Logging middleware example.
//!
//! Demonstrates how to:
//! - Record request start time via `GrpcContext`
//! - Log request path, response status, and latency
//! - Use `on_error` to log failures separately
//!
//! Run: `cargo run --example logging`

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use async_trait::async_trait;
use http::{Request, Response};
use http_body::Body;
use tonic::Status;
use tower_service::Service;

use myelin::prelude::*;

// ---------------------------------------------------------------------------
// Context types
// ---------------------------------------------------------------------------

struct RequestStart(Instant);
struct RequestPath(String);

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

struct LoggingMiddleware;

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
impl GrpcMiddleware for LoggingMiddleware {
    type Body = EmptyBody;

    async fn on_request(
        &self,
        req: Request<EmptyBody>,
        ctx: &mut GrpcContext,
    ) -> Result<Request<EmptyBody>, Status> {
        let path = req.uri().path().to_string();
        println!("  [log] --> {} {}", req.method(), path);

        ctx.insert(RequestStart(Instant::now()));
        ctx.insert(RequestPath(path));

        Ok(req)
    }

    async fn on_response(
        &self,
        res: Response<EmptyBody>,
        ctx: &GrpcContext,
    ) -> Result<Response<EmptyBody>, Status> {
        let elapsed = ctx
            .get::<RequestStart>()
            .map(|s| s.0.elapsed())
            .unwrap_or_default();
        let path = ctx
            .get::<RequestPath>()
            .map(|p| p.0.as_str())
            .unwrap_or("unknown");

        println!("  [log] <-- {} OK ({:?})", path, elapsed);
        Ok(res)
    }

    async fn on_error(&self, err: Status, ctx: &GrpcContext) -> Status {
        let elapsed = ctx
            .get::<RequestStart>()
            .map(|s| s.0.elapsed())
            .unwrap_or_default();
        let path = ctx
            .get::<RequestPath>()
            .map(|p| p.0.as_str())
            .unwrap_or("unknown");

        println!(
            "  [log] <-- {} ERROR code={:?} msg={:?} ({:?})",
            path,
            err.code(),
            err.message(),
            elapsed
        );
        err
    }
}

// ---------------------------------------------------------------------------
// Fake services
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct OkService;

impl Service<Request<EmptyBody>> for OkService {
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

#[derive(Clone)]
struct FailingService;

impl Service<Request<EmptyBody>> for FailingService {
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
                .header("grpc-status", "13") // INTERNAL
                .header("grpc-message", "database connection lost")
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
    let layer = MyelinLayer::new(LoggingMiddleware);

    // --- Successful request ---
    println!("Successful request:");
    let mut svc = tower_layer::Layer::layer(&layer, OkService);
    let req = Request::builder()
        .uri("/task.TaskService/GetTasks")
        .body(EmptyBody)
        .unwrap();
    let _ = svc.call(req).await;

    // --- Failed request (triggers on_error) ---
    println!("\nFailed request:");
    let mut svc = tower_layer::Layer::layer(&layer, FailingService);
    let req = Request::builder()
        .uri("/task.TaskService/CreateTask")
        .body(EmptyBody)
        .unwrap();
    let _ = svc.call(req).await;
}
