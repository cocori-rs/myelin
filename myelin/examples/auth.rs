//! Authentication middleware example.
//!
//! Demonstrates how to:
//! - Extract and validate a token from request headers
//! - Insert typed claims into `GrpcContext`
//! - Reject unauthenticated requests with `Status::unauthenticated`
//!
//! Run: `cargo run --example auth`

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use http::{Request, Response};
use http_body::Body;
use tonic::Status;
use tower_service::Service;

use myelin::prelude::*;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AuthClaims {
    user_id: u64,
    role: String,
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

struct AuthMiddleware;

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
impl GrpcMiddleware for AuthMiddleware {
    type Body = EmptyBody;

    async fn on_request(
        &self,
        req: Request<EmptyBody>,
        ctx: &mut GrpcContext,
    ) -> Result<Request<EmptyBody>, Status> {
        // Extract the authorization header.
        let token = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_start_matches("Bearer "))
            .ok_or_else(|| Status::unauthenticated("missing authorization header"))?;

        // In a real app this would verify a JWT via JWKS, database lookup, etc.
        // Here we accept any non-empty token for demonstration.
        if token.is_empty() {
            return Err(Status::unauthenticated("empty token"));
        }

        // Insert typed claims — downstream can access them without parsing strings.
        let claims = AuthClaims {
            user_id: 42,
            role: "admin".into(),
        };
        println!("  [auth] Authenticated user_id={} role={}", claims.user_id, claims.role);
        ctx.insert(claims);

        Ok(req)
    }

    async fn on_response(
        &self,
        res: Response<EmptyBody>,
        ctx: &GrpcContext,
    ) -> Result<Response<EmptyBody>, Status> {
        if let Some(claims) = ctx.get::<AuthClaims>() {
            println!("  [auth] Response for user_id={}", claims.user_id);
        }
        Ok(res)
    }
}

// ---------------------------------------------------------------------------
// Fake inner service (stands in for a real tonic service)
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
            println!("  [service] Handling request");
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
    let layer = MyelinLayer::new(AuthMiddleware);
    let mut svc = tower_layer::Layer::layer(&layer, EchoService);

    // --- Authenticated request ---
    println!("Request with valid token:");
    let req = Request::builder()
        .header("authorization", "Bearer my-jwt-token")
        .body(EmptyBody)
        .unwrap();
    let res = svc.call(req).await.unwrap();
    println!("  grpc-status: {}\n", res.headers().get("grpc-status").unwrap().to_str().unwrap());

    // --- Unauthenticated request ---
    println!("Request without token:");
    let req = Request::builder().body(EmptyBody).unwrap();
    let res = svc.call(req).await.unwrap();
    let status = res.headers().get("grpc-status").unwrap().to_str().unwrap();
    let message = res.headers().get("grpc-message").unwrap().to_str().unwrap();
    println!("  grpc-status: {} ({})", status, message);
}
