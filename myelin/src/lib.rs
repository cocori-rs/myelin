pub mod context;
pub mod error;
pub mod layer;
pub mod middleware;

pub mod prelude {
    pub use crate::context::GrpcContext;
    pub use crate::error::MyelinError;
    pub use crate::layer::{MyelinLayer, MyelinService};
    pub use crate::middleware::GrpcMiddleware;
}

// Re-export key types at crate root for convenience.
pub use context::GrpcContext;
pub use error::MyelinError;
pub use layer::{MyelinLayer, MyelinService};
pub use middleware::GrpcMiddleware;
