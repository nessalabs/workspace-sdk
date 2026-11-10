//! Receipt diagnostics contain compiled field names and JSON kinds, never
//! values: they reach the caller's event sink as
//! [`MachineEvent::ReceiptRejected`](super::MachineEvent::ReceiptRejected).
use super::{Boat, MachineEvent};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Which provider answer a rejected receipt came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    CreateBody,
    CreateDecode,
    CreateValidation,
    InfoBody,
    InfoValidation,
    InfoDecode,
}
impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::CreateBody => "create_body",
            Self::CreateDecode => "create_decode",
            Self::CreateValidation => "create_validation",
            Self::InfoBody => "info_body",
            Self::InfoValidation => "info_validation",
            Self::InfoDecode => "info_decode",
        }
    }
}

/// Where a receipt was refused: compiled names and the JSON kind of the
/// value found, never the value itself or a key the provider made up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: Stage,
    pub scope: &'static str,
    pub field: &'static str,
    pub problem: &'static str,
    pub observed_type: &'static str,
}
impl Diagnostic {
    pub(super) fn opaque(
        stage: Stage,
        scope: &'static str,
        field: &'static str,
        problem: &'static str,
    ) -> Self {
        Self {
            stage,
            scope,
            field,
            problem,
            observed_type: "not_collected",
        }
    }

    pub(super) fn field(
        stage: Stage,
        scope: &'static str,
        field: &'static str,
        problem: &'static str,
        value: Option<&Value>,
    ) -> Self {
        let observed_type = if field == "unknown" {
            "not_collected"
        } else {
            match value {
                None => "missing",
                Some(Value::Null) => "null",
                Some(Value::Bool(_)) => "boolean",
                Some(Value::String(_)) => "string",
                Some(Value::Array(_)) => "array",
                Some(Value::Object(_)) => "object",
                Some(Value::Number(number)) if number.is_f64() => "floating_number",
                Some(Value::Number(number)) => match number.as_u64() {
                    Some(value) if u32::try_from(value).is_ok() => "unsigned_u32",
                    Some(_) => "unsigned_wide",
                    None => "signed_integer",
                },
            }
        };
        Self {
            stage,
            scope,
            field,
            problem,
            observed_type,
        }
    }

    pub(super) fn emit(self, boat: &Boat) {
        boat.record(MachineEvent::ReceiptRejected(self));
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;

/// The SDK owns DTO decoding. Error paths are projected to compiled names;
/// neither its error message nor arbitrary map keys cross into diagnostics.
pub(super) fn decode<T: DeserializeOwned>(
    value: &Value,
    stage: Stage,
    sandbox_fields: &'static [&'static str],
) -> Result<T, Diagnostic> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let mut segments = error.path().iter();
        let (scope, field) = match segments.next() {
            Some(serde_path_to_error::Segment::Map { key }) if key == "sandbox" => {
                let field = match segments.next() {
                    Some(serde_path_to_error::Segment::Map { key }) => sandbox_fields
                        .iter()
                        .copied()
                        .find(|field| *field == key)
                        .unwrap_or("unknown"),
                    _ => "sandbox",
                };
                ("sandbox", field)
            }
            Some(serde_path_to_error::Segment::Map { key }) => (
                "envelope",
                match key.as_str() {
                    "ok" => "ok",
                    "type" => "type",
                    "status" => "status",
                    "ttlSeconds" => "ttlSeconds",
                    "giftLimitNotice" => "giftLimitNotice",
                    _ => "unknown",
                },
            ),
            _ => ("body", "body"),
        };
        let observed = match scope {
            "sandbox" if field == "sandbox" => value.get("sandbox"),
            "sandbox" => value.get("sandbox").and_then(|v| v.get(field)),
            "envelope" => value.get(field),
            _ => Some(value),
        };
        Diagnostic::field(stage, scope, field, "decode", observed)
    })
}
