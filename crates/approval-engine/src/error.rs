//! Approval / attestation error type.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("approval '{0}' not found")]
    NotFound(String),
    #[error("approval '{0}' is already resolved")]
    AlreadyResolved(String),
    #[error("an agent cannot approve its own request '{0}'")]
    SelfApproval(String),
    #[error("cannot attest a pending approval '{0}'")]
    Pending(String),
}
