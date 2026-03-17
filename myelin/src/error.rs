use tonic::Status;

#[derive(Debug, thiserror::Error)]
pub enum MyelinError {
    #[error("middleware error: {0}")]
    Middleware(#[from] tonic::Status),

    #[error("context error: {0}")]
    Context(String),
}

impl From<MyelinError> for Status {
    fn from(e: MyelinError) -> Self {
        match e {
            MyelinError::Middleware(s) => s,
            MyelinError::Context(msg) => Status::internal(msg),
        }
    }
}
