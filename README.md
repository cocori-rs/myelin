# myelin

Type-safe, async-first middleware layer for [tonic](https://github.com/hyperium/tonic) gRPC services.

Named after the myelin sheath — the biological insulating layer that transmits nerve signals safely and at high speed.

## The Problem

tonic's built-in interceptor is synchronous and request-only. You can't call async services (Redis, DB, JWKS endpoints), modify responses, or pass typed data to downstream layers without resorting to stringly-typed HTTP headers.

## What myelin Provides

- **Async hooks** — `on_request`, `on_response`, and `on_error` are all `async fn`
- **Type-safe context** — `GrpcContext` is a `TypeId`-keyed map, so middleware layers share data with compile-time type safety instead of header strings
- **Dedicated error hook** — handle service errors in one place instead of pattern-matching inside `call()`
- **Tower integration** — `MyelinLayer` / `MyelinService` plug directly into `Server::builder().layer()`

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
myelin = "0.1"
```

### Define a middleware

```rust
use async_trait::async_trait;
use myelin::prelude::*;
use tonic::Status;

struct AuthMiddleware;

#[async_trait]
impl GrpcMiddleware for AuthMiddleware {
    type Body = tonic::body::BoxBody;

    async fn on_request(
        &self,
        req: http::Request<Self::Body>,
        ctx: &mut GrpcContext,
    ) -> Result<http::Request<Self::Body>, Status> {
        // Verify token, insert typed claims into context
        ctx.insert(AuthClaims { user_id: 42 });
        Ok(req)
    }

    async fn on_response(
        &self,
        res: http::Response<Self::Body>,
        _ctx: &GrpcContext,
    ) -> Result<http::Response<Self::Body>, Status> {
        Ok(res)
    }
}
```

### Apply to a tonic server

```rust
Server::builder()
    .layer(MyelinLayer::new(AuthMiddleware))
    .add_service(MyServiceServer::new(my_service))
    .serve(addr)
    .await?;
```

### Access context in your service

```rust
let claims = ctx.get::<AuthClaims>()
    .ok_or(Status::unauthenticated("not authenticated"))?;
```

## Scope (v0.1)

- Server-side middleware only
- Unary RPC support
- `GrpcContext`, `GrpcMiddleware` trait, Tower `Layer`/`Service` integration

Streaming RPC, client-side middleware, and proc-macros are planned for future releases.

## License

MIT
