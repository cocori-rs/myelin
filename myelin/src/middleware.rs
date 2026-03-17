use async_trait::async_trait;
use http::Response;
use http_body::Body;
use tonic::Status;

use crate::context::GrpcContext;

/// Async middleware trait for tonic gRPC services.
///
/// Implement this trait to hook into the request/response lifecycle with full
/// async support and type-safe context sharing via [`GrpcContext`].
#[async_trait]
pub trait GrpcMiddleware: Send + Sync + 'static {
    /// The body type used in HTTP requests and responses.
    ///
    /// For tonic services this is typically `tonic::body::BoxBody`.
    type Body: Body + Send + 'static;

    /// Called before the request reaches the inner service.
    /// Insert data into `ctx` here.
    async fn on_request(
        &self,
        req: http::Request<Self::Body>,
        ctx: &mut GrpcContext,
    ) -> Result<http::Request<Self::Body>, Status>;

    /// Called after the inner service returns a successful response.
    /// Modify or observe the response here.
    async fn on_response(
        &self,
        res: Response<Self::Body>,
        ctx: &GrpcContext,
    ) -> Result<Response<Self::Body>, Status>;

    /// Called when the inner service returns an error.
    /// Default implementation passes the error through unchanged.
    async fn on_error(&self, err: Status, _ctx: &GrpcContext) -> Status {
        err
    }
}
