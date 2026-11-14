//! Why an Agent37 machine operation did not succeed.

/// Why a machine operation failed. It never holds the API's own words:
/// they are not needed to decide what to do, and are not logged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Agent37Failure {
    /// A request this SDK would not send, or the API refused as malformed.
    #[error("invalid input")]
    InvalidInput,
    /// The key was refused.
    #[error("the Agent37 key was refused")]
    Unauthorized,
    /// The template or instance does not exist in the workspace.
    #[error("missing")]
    Missing,
    /// The wallet or the instance limit does not allow it.
    #[error("the Agent37 workspace's balance or instance limit does not allow it")]
    Funds,
    /// The instance is not the machine this was asked for.
    #[error("not admitted: {0}")]
    AdmissionRejected(AdmissionRefusal),
    /// A creation may or may not have made an instance; asking again with
    /// the same key finds it if it did.
    #[error("provisioning uncertain")]
    ProvisioningUncertain,
    /// The instance did not become ready within the budget.
    #[error("timed out")]
    Timeout,
    /// Anything else the API or the network did.
    #[error("Agent37 failed")]
    Provider,
}

/// Why an instance is not admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionRefusal {
    /// It runs another template, or another pin of it.
    #[error("wrong template")]
    WrongTemplate,
    /// It names no image digest, so what it runs is not known.
    #[error("no image digest")]
    MissingDigest,
    /// Its image changed since it was admitted.
    #[error("image changed")]
    ImageChanged,
    /// It carries another workspace's key, or none.
    #[error("not this workspace's")]
    WrongOwner,
    /// It failed or is being deleted.
    #[error("failed")]
    Failed,
}
