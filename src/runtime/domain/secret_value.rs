//! The explicit in-memory boundary for resolved credential material.
//!
//! This value prevents accidental serialization and debug disclosure; it does not
//! claim encrypted or zeroizing storage and does not redact workload output.

use std::fmt;

/// Transient credential material with an intentionally explicit exposure boundary.
///
/// This owned value has no serde implementation, and debug output is redacted.
/// It is separate from the persistable [`crate::runtime::CredentialRef`] that names it.
/// The wrapper does not encrypt memory, zeroize on drop, or redact workload output;
/// adapters must keep the exposed material out of ordinary command records.
///
/// ```compile_fail
/// let secret = workspace_sdk::runtime::SecretValue::new("private");
/// serde_json::to_string(&secret).unwrap();
/// ```
#[derive(Clone)]
pub struct SecretValue(String);

impl SecretValue {
    /// Wrap caller-supplied material without validating or persisting it.
    /// Preparation separately rejects empty or NUL-bearing process credentials.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the underlying material for explicit private transport or validation.
    /// Host adapters must not copy it into ordinary arguments, diagnostics, or receipts.
    pub fn expose(&self) -> &str {
        &self.0
    }
    /// Whether the wrapped string contains no bytes. This does not establish
    /// credential validity, expiry, or upstream access.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::SecretValue;
    #[test]
    fn diagnostic_format_is_redacted_while_adapter_access_is_explicit() {
        let value = SecretValue::new("fixture-secret");
        assert_eq!(format!("{value:?}"), "[REDACTED]");
        assert_eq!(value.expose(), "fixture-secret");
    }
}
