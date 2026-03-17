//! Error-handling middleware example.
//!
//! Demonstrates how to:
//! - Use `on_error` to intercept and transform service errors
//! - Sanitize internal error details before they reach the client
//! - Map error codes (e.g. INTERNAL → UNAVAILABLE for retryable errors)
//!
//! Run: `cargo run --example error_handler`

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use http::{Request, Response};
use http_body::Body;
use tonic::{Code, Status};
use tower_service::Service;

use myelin::prelude::*;

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

struct ErrorHandlerMiddleware;

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
impl GrpcMiddleware for ErrorHandlerMiddleware {
    type Body = EmptyBody;

    async fn on_request(
        &self,
        req: Request<EmptyBody>,
        _ctx: &mut GrpcContext,
    ) -> Result<Request<EmptyBody>, Status> {
        Ok(req)
    }

    async fn on_response(
        &self,
        res: Response<EmptyBody>,
        _ctx: &GrpcContext,
    ) -> Result<Response<EmptyBody>, Status> {
        Ok(res)
    }

    async fn on_error(&self, err: Status, _ctx: &GrpcContext) -> Status {
        println!("  [error_handler] Intercepted: code={:?} msg={:?}", err.code(), err.message());

        match err.code() {
            // Sanitize internal errors — don't leak implementation details.
            Code::Internal => {
                println!("  [error_handler] Sanitizing INTERNAL error");
                Status::internal("an internal error occurred")
            }
            // Map certain transient errors to UNAVAILABLE so clients know to retry.
            Code::DeadlineExceeded => {
                println!("  [error_handler] Mapping DEADLINE_EXCEEDED → UNAVAILABLE");
                Status::unavailable("service temporarily unavailable, please retry")
            }
            // Pass everything else through unchanged.
            _ => err,
        }
    }
}

// ---------------------------------------------------------------------------
// Fake services that return different errors
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct InternalErrorService;

impl Service<Request<EmptyBody>> for InternalErrorService {
    type Response = Response<EmptyBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request<EmptyBody>) -> Self::Future {
        Box::pin(async {
            // Simulate a leaked database error message.
            Ok(Response::builder()
                .status(200)
                .header("grpc-status", "13") // INTERNAL
                .header("grpc-message", "pq: duplicate key violates unique constraint \"users_email_key\"")
                .body(EmptyBody)
                .unwrap())
        })
    }
}

#[derive(Clone)]
struct DeadlineExceededService;

impl Service<Request<EmptyBody>> for DeadlineExceededService {
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
                .header("grpc-status", "4") // DEADLINE_EXCEEDED
                .header("grpc-message", "upstream timed out after 30s")
                .body(EmptyBody)
                .unwrap())
        })
    }
}

#[derive(Clone)]
struct NotFoundService;

impl Service<Request<EmptyBody>> for NotFoundService {
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
                .header("grpc-status", "5") // NOT_FOUND
                .header("grpc-message", "task not found")
                .body(EmptyBody)
                .unwrap())
        })
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn print_response(res: &Response<EmptyBody>) {
    let status = res.headers().get("grpc-status").unwrap().to_str().unwrap();
    let msg = res.headers().get("grpc-message").unwrap().to_str().unwrap();
    println!("  Result: grpc-status={} msg={:?}\n", status, msg);
}

#[tokio::main]
async fn main() {
    let layer = MyelinLayer::new(ErrorHandlerMiddleware);

    // --- INTERNAL error: message gets sanitized ---
    println!("1) INTERNAL error (leaked DB details):");
    let mut svc = tower_layer::Layer::layer(&layer, InternalErrorService);
    let req = Request::builder().body(EmptyBody).unwrap();
    let res = svc.call(req).await.unwrap();
    print_response(&res);

    // --- DEADLINE_EXCEEDED: remapped to UNAVAILABLE ---
    println!("2) DEADLINE_EXCEEDED (remapped to UNAVAILABLE):");
    let mut svc = tower_layer::Layer::layer(&layer, DeadlineExceededService);
    let req = Request::builder().body(EmptyBody).unwrap();
    let res = svc.call(req).await.unwrap();
    print_response(&res);

    // --- NOT_FOUND: passed through unchanged ---
    println!("3) NOT_FOUND (passed through):");
    let mut svc = tower_layer::Layer::layer(&layer, NotFoundService);
    let req = Request::builder().body(EmptyBody).unwrap();
    let res = svc.call(req).await.unwrap();
    print_response(&res);
}
