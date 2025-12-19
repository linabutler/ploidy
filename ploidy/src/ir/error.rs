use crate::parse::path::BadPath;

#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("can't generate code for an operation without an ID")]
    NoOperationId,
    #[error("operation has invalid path")]
    BadOperationPath(#[from] BadPath),
}
