use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Request, Response};
use http_body::Body;
use tonic::Status;
use tower_layer::Layer;
use tower_service::Service;

use crate::context::GrpcContext;
use crate::middleware::GrpcMiddleware;

/// Tower [`Layer`] that wraps a [`GrpcMiddleware`] implementor.
///
/// # Usage
///
/// ```ignore
/// Server::builder()
///     .layer(MyelinLayer::new(AuthMiddleware::new()))
///     .add_service(my_service)
///     .serve(addr)
///     .await?;
/// ```
pub struct MyelinLayer<M> {
    middleware: Arc<M>,
}

impl<M> MyelinLayer<M>
where
    M: GrpcMiddleware,
{
    pub fn new(middleware: M) -> Self {
        Self {
            middleware: Arc::new(middleware),
        }
    }
}

impl<M> Clone for MyelinLayer<M> {
    fn clone(&self) -> Self {
        Self {
            middleware: Arc::clone(&self.middleware),
        }
    }
}

impl<M, S> Layer<S> for MyelinLayer<M>
where
    M: GrpcMiddleware,
{
    type Service = MyelinService<M, S>;

    fn layer(&self, inner: S) -> Self::Service {
        MyelinService {
            middleware: Arc::clone(&self.middleware),
            inner,
        }
    }
}

/// Tower [`Service`] produced by [`MyelinLayer`].
///
/// Delegates to the wrapped [`GrpcMiddleware`] for request/response/error
/// processing and forwards to the inner service.
pub struct MyelinService<M, S> {
    middleware: Arc<M>,
    inner: S,
}

impl<M, S> Clone for MyelinService<M, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            middleware: Arc::clone(&self.middleware),
            inner: self.inner.clone(),
        }
    }
}

impl<M, S, B> Service<Request<B>> for MyelinService<M, S>
where
    M: GrpcMiddleware<Body = B>,
    B: Body + Send + Default + 'static,
    S: Service<Request<B>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = Response<B>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let middleware = Arc::clone(&self.middleware);
        let mut inner = self.inner.clone();
        // Swap so the ready clone becomes `self` for the next call.
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move {
            let mut ctx = GrpcContext::new();

            // --- on_request ---
            let req = match middleware.on_request(req, &mut ctx).await {
                Ok(req) => req,
                Err(status) => {
                    return Ok(into_http_response(status));
                }
            };

            // --- inner service ---
            let result: Result<Response<B>, S::Error> = inner.call(req).await;

            match result {
                Ok(res) => {
                    // Inspect grpc-status to decide whether this is an error.
                    let grpc_status = res
                        .headers()
                        .get("grpc-status")
                        .and_then(|v: &http::HeaderValue| v.to_str().ok())
                        .and_then(|s: &str| s.parse::<i32>().ok())
                        .unwrap_or(0);

                    if grpc_status == 0 {
                        // --- on_response ---
                        match middleware.on_response(res, &ctx).await {
                            Ok(res) => Ok(res),
                            Err(status) => Ok(into_http_response(status)),
                        }
                    } else {
                        // tonic encodes errors as HTTP 200 with grpc-status header.
                        // Extract the status and pass through on_error.
                        let status = Status::from_header_map(res.headers())
                            .unwrap_or_else(|| Status::unknown("unknown error"));
                        let status = middleware.on_error(status, &ctx).await;
                        Ok(into_http_response(status))
                    }
                }
                Err(err) => Err(err),
            }
        })
    }
}

/// Convert a `tonic::Status` into an HTTP response with gRPC trailers-only encoding.
fn into_http_response<B: Body + Default>(status: Status) -> Response<B> {
    let code = status.code();
    let message = status.message().to_string();

    // gRPC always uses HTTP 200; the real status is in trailers/headers.
    Response::builder()
        .status(http::StatusCode::OK)
        .header("content-type", "application/grpc")
        .header("grpc-status", (code as i32).to_string())
        .header("grpc-message", message)
        .body(B::default())
        .expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GrpcContext;
    use crate::middleware::GrpcMiddleware;
    use async_trait::async_trait;
    use http_body::Body;
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tower_layer::Layer;

    // -- Minimal body type for testing --

    #[derive(Default)]
    struct TestBody;

    impl Body for TestBody {
        type Data = bytes::Bytes;
        type Error = Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(None)
        }
    }

    // -- Test middleware that inserts a value into context --

    struct TestMiddleware;

    #[async_trait]
    impl GrpcMiddleware for TestMiddleware {
        type Body = TestBody;

        async fn on_request(
            &self,
            req: Request<TestBody>,
            ctx: &mut GrpcContext,
        ) -> Result<Request<TestBody>, Status> {
            ctx.insert(42u32);
            Ok(req)
        }

        async fn on_response(
            &self,
            res: Response<TestBody>,
            ctx: &GrpcContext,
        ) -> Result<Response<TestBody>, Status> {
            assert_eq!(ctx.get::<u32>(), Some(&42));
            Ok(res)
        }
    }

    // -- Minimal inner service for testing --

    #[derive(Clone)]
    struct OkService;

    impl Service<Request<TestBody>> for OkService {
        type Response = Response<TestBody>;
        type Error = Infallible;
        type Future = Pin<
            Box<dyn std::future::Future<Output = Result<Response<TestBody>, Infallible>> + Send>,
        >;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: Request<TestBody>) -> Self::Future {
            Box::pin(async {
                Ok(Response::builder()
                    .status(200)
                    .header("grpc-status", "0")
                    .body(TestBody)
                    .unwrap())
            })
        }
    }

    #[tokio::test]
    async fn layer_produces_service_and_calls_hooks() {
        let layer = MyelinLayer::new(TestMiddleware);
        let mut svc = layer.layer(OkService);

        let req = Request::builder().body(TestBody).unwrap();

        let res = svc.call(req).await.unwrap();
        assert_eq!(res.status(), 200);
    }

    // -- Test middleware that rejects on_request --

    struct RejectMiddleware;

    #[async_trait]
    impl GrpcMiddleware for RejectMiddleware {
        type Body = TestBody;

        async fn on_request(
            &self,
            _req: Request<TestBody>,
            _ctx: &mut GrpcContext,
        ) -> Result<Request<TestBody>, Status> {
            Err(Status::unauthenticated("denied"))
        }

        async fn on_response(
            &self,
            res: Response<TestBody>,
            _ctx: &GrpcContext,
        ) -> Result<Response<TestBody>, Status> {
            Ok(res)
        }
    }

    #[tokio::test]
    async fn on_request_rejection_returns_error_response() {
        let layer = MyelinLayer::new(RejectMiddleware);
        let mut svc = layer.layer(OkService);

        let req = Request::builder().body(TestBody).unwrap();

        let res = svc.call(req).await.unwrap();
        let grpc_status = res
            .headers()
            .get("grpc-status")
            .unwrap()
            .to_str()
            .unwrap();
        // UNAUTHENTICATED = 16
        assert_eq!(grpc_status, "16");
    }

    // -- Test on_error hook --

    #[derive(Clone)]
    struct ErrorService;

    impl Service<Request<TestBody>> for ErrorService {
        type Response = Response<TestBody>;
        type Error = Infallible;
        type Future = Pin<
            Box<dyn std::future::Future<Output = Result<Response<TestBody>, Infallible>> + Send>,
        >;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: Request<TestBody>) -> Self::Future {
            Box::pin(async {
                Ok(Response::builder()
                    .status(200)
                    .header("grpc-status", "13") // INTERNAL
                    .header("grpc-message", "something broke")
                    .body(TestBody)
                    .unwrap())
            })
        }
    }

    struct ErrorRewriteMiddleware;

    #[async_trait]
    impl GrpcMiddleware for ErrorRewriteMiddleware {
        type Body = TestBody;

        async fn on_request(
            &self,
            req: Request<TestBody>,
            _ctx: &mut GrpcContext,
        ) -> Result<Request<TestBody>, Status> {
            Ok(req)
        }

        async fn on_response(
            &self,
            res: Response<TestBody>,
            _ctx: &GrpcContext,
        ) -> Result<Response<TestBody>, Status> {
            Ok(res)
        }

        async fn on_error(&self, _err: Status, _ctx: &GrpcContext) -> Status {
            Status::unavailable("rewritten")
        }
    }

    #[tokio::test]
    async fn on_error_hook_is_invoked() {
        let layer = MyelinLayer::new(ErrorRewriteMiddleware);
        let mut svc = layer.layer(ErrorService);

        let req = Request::builder().body(TestBody).unwrap();

        let res = svc.call(req).await.unwrap();
        let grpc_status = res
            .headers()
            .get("grpc-status")
            .unwrap()
            .to_str()
            .unwrap();
        // UNAVAILABLE = 14
        assert_eq!(grpc_status, "14");
    }
}
