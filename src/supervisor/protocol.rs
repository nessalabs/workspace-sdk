//! Typed wire contract between provider transports and the Rust supervisor.
//!
//! Request values are serialized only when crossing a process/API boundary.
//! Credentials are transient fields, never part of persisted run records.
use crate::{
    CommandSpec, Error, HostResources, InputOutcome, OutputBytesPage, OutputCursor, OutputPage,
    Ownership, Result, RunId, RunState, SecretEnvironment, SecretInput, TerminalSize, WorkspaceId,
    WorkspacePath,
};
#[cfg(feature = "portability")]
use crate::{TreeExportReceipt, TreeImportReceipt, TreeTransfer};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use uuid::Uuid;

/// Identity and absolute paths that every operation must verify against owner.json.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorContext {
    pub workspace_id: WorkspaceId,
    pub root: String,
    pub control: String,
    pub ownership: Ownership,
}
/// Chooses a validated project or private-control working directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scope", content = "path", rename_all = "snake_case")]
pub enum WorkingDirectory {
    Project(WorkspacePath),
    Control(WorkspacePath),
}
/// Transient launch inputs; Debug redacts credential values and private stdin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorExecution {
    pub command: CommandSpec,
    #[serde(with = "secret_environment")]
    pub secrets: SecretEnvironment,
    #[serde(default, with = "secret_input")]
    pub private_stdin: Option<SecretInput>,
    pub directory: WorkingDirectory,
}
/// Operations understood by protocol version 1. Variant names describe their intent.
///
/// The file-browser and tree-transfer operations exist only with the
/// `portability` feature. A supervisor answers an operation name it does not
/// know with an `Unsupported` failure, so a client built with more features
/// than its supervisor gets a clear refusal; see [`SupervisorRequest::decode`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", content = "parameters", rename_all = "snake_case")]
pub enum SupervisorOperation {
    Initialize {
        create_project: bool,
    },
    Ready,
    ReadFile {
        path: WorkspacePath,
        offset: u64,
        limit: u32,
    },
    WriteFile {
        path: WorkspacePath,
        offset: u64,
        #[serde(with = "binary")]
        bytes: Vec<u8>,
        truncate: bool,
        executable: bool,
    },
    #[cfg(feature = "portability")]
    ListDirectory(crate::ListDirectory),
    #[cfg(feature = "portability")]
    ReadVersionedFile(crate::ReadVersionedFile),
    #[cfg(feature = "portability")]
    PublishFile(crate::PublishFile),
    Start {
        id: RunId,
        execution: Box<SupervisorExecution>,
        output_limit_bytes: u64,
    },
    StartTerminal {
        id: RunId,
        execution: Box<SupervisorExecution>,
        output_limit_bytes: u64,
        size: TerminalSize,
    },
    TerminalInput {
        id: RunId,
        #[serde(with = "secret_input")]
        input: Option<SecretInput>,
    },
    TerminalResize {
        id: RunId,
        size: TerminalSize,
    },
    Inspect {
        id: RunId,
    },
    Cancel {
        id: RunId,
    },
    Output {
        id: RunId,
        cursor: OutputCursor,
        limit: u32,
    },
    OutputBytes {
        id: RunId,
        cursor: OutputCursor,
        limit: u32,
    },
    StopOwnedRuns,
    DestroyOwnedFiles,
    #[cfg(feature = "portability")]
    ExportTree {
        transfer: TreeTransfer,
    },
    #[cfg(feature = "portability")]
    ImportTree {
        transfer: TreeTransfer,
    },
    #[cfg(feature = "portability")]
    RemoveTransfer {
        transfer: TreeTransfer,
    },
    EnsurePrivateDirectory {
        path: WorkspacePath,
    },
    WritePrivateFile {
        path: WorkspacePath,
        #[serde(with = "binary")]
        bytes: Vec<u8>,
    },
    InspectResources,
    /// Reserve a private, exclusive request file before a provider uploads input.
    PrepareRequest {
        id: Uuid,
    },
    /// Remove one transient request after failed or completed provider transport.
    RemoveRequest {
        id: Uuid,
    },
    /// An operation name this build does not know. Serde yields it for such a
    /// name without parameters; [`SupervisorRequest::decode`] refuses it, and
    /// any unknown name, as `Unsupported`. It cannot be serialized or sent.
    #[serde(other, skip_serializing)]
    Unknown,
}
impl SupervisorOperation {
    /// Payload-bearing operations require private provider transport. Exhaustive
    /// classification prevents a new input operation from falling into command history.
    #[cfg(any(feature = "boat", feature = "space"))]
    pub(crate) fn requires_private_transport(&self) -> bool {
        match self {
            Self::Start { .. }
            | Self::StartTerminal { .. }
            | Self::TerminalInput { .. }
            | Self::WriteFile { .. }
            | Self::WritePrivateFile { .. } => true,
            #[cfg(feature = "portability")]
            Self::PublishFile(..) => true,
            #[cfg(feature = "portability")]
            Self::ListDirectory(..)
            | Self::ReadVersionedFile(..)
            | Self::ExportTree { .. }
            | Self::ImportTree { .. }
            | Self::RemoveTransfer { .. } => false,
            Self::Initialize { .. }
            | Self::Ready
            | Self::ReadFile { .. }
            | Self::TerminalResize { .. }
            | Self::Inspect { .. }
            | Self::Cancel { .. }
            | Self::Output { .. }
            | Self::OutputBytes { .. }
            | Self::StopOwnedRuns
            | Self::DestroyOwnedFiles
            | Self::EnsurePrivateDirectory { .. }
            | Self::InspectResources
            | Self::PrepareRequest { .. }
            | Self::RemoveRequest { .. }
            | Self::Unknown => false,
        }
    }
}
/// A complete, versioned request. The operation is explicit rather than an opaque map.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRequest {
    pub version: u32,
    pub context: SupervisorContext,
    pub operation: SupervisorOperation,
}
impl SupervisorRequest {
    pub fn new(context: SupervisorContext, operation: SupervisorOperation) -> Self {
        Self {
            version: 1,
            context,
            operation,
        }
    }
    /// Decode one request. A request naming an operation this build does not
    /// know is `Unsupported`, not a schema error: a client built with more
    /// features than its supervisor gets a clear refusal. A known operation
    /// with bad parameters, or anything that is not a request, is `InvalidInput`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let request = serde_json::from_slice::<Self>(bytes);
        match request {
            Ok(request) if matches!(request.operation, SupervisorOperation::Unknown) => {
                Err(unknown_operation(bytes))
            }
            Ok(request) => Ok(request),
            Err(_) if names_unknown_operation(bytes) => Err(unknown_operation(bytes)),
            Err(_) => Err(Error::InvalidInput(
                "invalid supervisor request schema".into(),
            )),
        }
    }
}
/// The operation name alone, so an unknown one is recognized whatever its parameters.
#[derive(Deserialize)]
struct OperationName {
    operation: OperationTag,
}
#[derive(Deserialize)]
struct OperationTag {
    operation: String,
}
fn operation_name(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<OperationName>(bytes)
        .ok()
        .map(|request| request.operation.operation)
}
fn names_unknown_operation(bytes: &[u8]) -> bool {
    operation_name(bytes).is_some_and(|name| {
        matches!(
            serde_json::from_value(serde_json::json!({ "operation": name })),
            Ok(SupervisorOperation::Unknown)
        )
    })
}
fn unknown_operation(bytes: &[u8]) -> Error {
    // Echo only a plausible operation name; the request is otherwise untrusted.
    let name = operation_name(bytes)
        .filter(|name| {
            name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
        .unwrap_or_else(|| "?".into());
    Error::Unsupported(format!(
        "supervisor operation `{name}` is not in this supervisor build; deploy a supervisor built with the features this client uses"
    ))
}
/// Typed file transfer response. Bytes use base64 only at the serialization boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferredFileChunk {
    #[serde(with = "binary")]
    pub bytes: Vec<u8>,
    pub next_offset: u64,
    pub eof: bool,
}
/// Binary output is encoded only at the supervisor transport boundary.
#[derive(Clone, Serialize, Deserialize)]
pub struct TransferredOutputBytes {
    #[serde(with = "binary")]
    pub stdout: Vec<u8>,
    #[serde(with = "binary")]
    pub stderr: Vec<u8>,
    pub next: OutputCursor,
    pub gap: bool,
    pub complete: bool,
}
impl From<OutputBytesPage> for TransferredOutputBytes {
    fn from(page: OutputBytesPage) -> Self {
        Self {
            stdout: page.stdout,
            stderr: page.stderr,
            next: page.next,
            gap: page.gap,
            complete: page.complete,
        }
    }
}
impl From<TransferredOutputBytes> for OutputBytesPage {
    fn from(page: TransferredOutputBytes) -> Self {
        Self {
            stdout: page.stdout,
            stderr: page.stderr,
            next: page.next,
            gap: page.gap,
            complete: page.complete,
        }
    }
}
impl std::fmt::Debug for TransferredOutputBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferredOutputBytes")
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("next", &self.next)
            .field("gap", &self.gap)
            .field("complete", &self.complete)
            .finish()
    }
}
/// A successful operation result, carrying its concrete domain type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SupervisorValue {
    Empty,
    #[cfg(feature = "portability")]
    DirectoryPage(crate::DirectoryPage),
    #[cfg(feature = "portability")]
    VersionedFileChunk(crate::VersionedFileChunk),
    #[cfg(feature = "portability")]
    PublishOutcome(crate::PublishOutcome),
    /// Target rejection before publication; transport failures remain Unknown.
    #[cfg(feature = "portability")]
    PublishRejected(SupervisorFault),
    Run(RunState),
    Input(InputOutcome),
    /// Definitive rejection established before any input could be dispatched.
    InputRejected(SupervisorFault),
    Output(OutputPage),
    OutputBytes(TransferredOutputBytes),
    FileChunk(TransferredFileChunk),
    #[cfg(feature = "portability")]
    TreeExport(TreeExportReceipt),
    #[cfg(feature = "portability")]
    TreeImport(TreeImportReceipt),
    Resources(HostResources),
}
impl SupervisorValue {
    pub fn empty(self) -> Result<()> {
        match self {
            Self::Empty => Ok(()),
            _ => Err(unexpected()),
        }
    }
    pub fn run(self) -> Result<RunState> {
        match self {
            Self::Run(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    pub fn input(self) -> Result<InputOutcome> {
        match self {
            Self::Input(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    pub fn output(self) -> Result<OutputPage> {
        match self {
            Self::Output(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    pub fn output_bytes(self) -> Result<OutputBytesPage> {
        match self {
            Self::OutputBytes(value) => Ok(value.into()),
            _ => Err(unexpected()),
        }
    }
    pub fn file_chunk(self) -> Result<TransferredFileChunk> {
        match self {
            Self::FileChunk(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    #[cfg(feature = "portability")]
    pub fn tree_export(self) -> Result<TreeExportReceipt> {
        match self {
            Self::TreeExport(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    #[cfg(feature = "portability")]
    pub fn tree_import(self) -> Result<TreeImportReceipt> {
        match self {
            Self::TreeImport(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    pub fn resources(self) -> Result<HostResources> {
        match self {
            Self::Resources(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
}
fn unexpected() -> Error {
    Error::State("supervisor returned a result for a different operation".into())
}
/// Protocol failures deliberately carry bounded, non-secret diagnostic messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorFaultKind {
    InvalidInput,
    Conflict,
    Unsupported,
    NotFound,
    State,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupervisorFault {
    pub kind: SupervisorFaultKind,
    pub message: String,
}
impl SupervisorFault {
    pub fn from_error(error: Error) -> Self {
        let kind = match &error {
            Error::InvalidInput(_) => SupervisorFaultKind::InvalidInput,
            Error::Conflict(_) => SupervisorFaultKind::Conflict,
            Error::Unsupported(_) => SupervisorFaultKind::Unsupported,
            Error::NotFound(_) => SupervisorFaultKind::NotFound,
            Error::Io(error) if error.kind() == ErrorKind::NotFound => {
                SupervisorFaultKind::NotFound
            }
            _ => SupervisorFaultKind::State,
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
    pub fn into_error(self) -> Error {
        match self.kind {
            SupervisorFaultKind::InvalidInput => Error::InvalidInput(self.message),
            SupervisorFaultKind::Conflict => Error::Conflict(self.message),
            SupervisorFaultKind::Unsupported => Error::Unsupported(self.message),
            SupervisorFaultKind::NotFound => Error::NotFound(self.message),
            SupervisorFaultKind::State => Error::State(self.message),
        }
    }
}
/// One response is emitted for each request; workload output has separate cursor APIs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum SupervisorResponse {
    Success(SupervisorValue),
    Failure(SupervisorFault),
}
impl SupervisorResponse {
    pub fn from_result(result: Result<SupervisorValue>) -> Self {
        match result {
            Ok(value) => Self::Success(value),
            Err(error) => Self::Failure(SupervisorFault::from_error(error)),
        }
    }
    pub fn into_result(self) -> Result<SupervisorValue> {
        match self {
            Self::Success(value) => Ok(value),
            Self::Failure(fault) => Err(fault.into_error()),
        }
    }
}
mod binary {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{de::Error as DeserializeError, Deserialize, Deserializer, Serializer};
    use std::result::Result;
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let value = String::deserialize(deserializer)?;
        STANDARD.decode(value).map_err(DeserializeError::custom)
    }
}
mod secret_environment {
    use crate::SecretEnvironment;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{collections::BTreeMap, result::Result};
    pub fn serialize<S: Serializer>(
        value: &SecretEnvironment,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.0.serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SecretEnvironment, D::Error> {
        Ok(SecretEnvironment(BTreeMap::deserialize(deserializer)?))
    }
}
pub(super) mod secret_input {
    use crate::SecretInput;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{de::Error as DeserializeError, Deserialize, Deserializer, Serialize, Serializer};
    use std::result::Result;
    pub fn serialize<S: Serializer>(
        value: &Option<SecretInput>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .as_ref()
            .map(|value| STANDARD.encode(value.expose()))
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<SecretInput>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|value| {
                STANDARD
                    .decode(value)
                    .map(SecretInput::new)
                    .map_err(DeserializeError::custom)
            })
            .transpose()
    }
}

#[cfg(test)]
mod output_bytes_wire_tests {
    use super::*;

    #[test]
    fn byte_output_wire_preserves_binary_and_rejects_malformed_base64() {
        let source: Vec<u8> = (0..=255).collect();
        let wire = TransferredOutputBytes::from(OutputBytesPage {
            stdout: source.clone(),
            stderr: source.iter().rev().copied().collect(),
            next: OutputCursor {
                stdout: 256,
                stderr: 512,
            },
            gap: true,
            complete: false,
        });
        let mut encoded = serde_json::to_value(&wire).unwrap();
        assert!(encoded["stdout"].is_string());
        let decoded: TransferredOutputBytes = serde_json::from_value(encoded.clone()).unwrap();
        let page = SupervisorValue::OutputBytes(decoded)
            .output_bytes()
            .unwrap();
        assert_eq!(page.stdout, source);
        assert_eq!(page.stderr, (0..=255).rev().collect::<Vec<u8>>());
        assert_eq!(page.next.stdout, 256);
        assert_eq!(page.next.stderr, 512);
        assert!(page.gap);
        assert!(!page.complete);
        for stream in ["stdout", "stderr"] {
            let original = encoded[stream].clone();
            encoded[stream] = serde_json::json!("not valid base64!");
            assert!(serde_json::from_value::<TransferredOutputBytes>(encoded.clone()).is_err());
            encoded[stream] = original;
        }
        let private = TransferredOutputBytes::from(OutputBytesPage {
            stdout: b"private-output-marker".to_vec(),
            stderr: b"private-error-marker".to_vec(),
            next: OutputCursor::default(),
            gap: false,
            complete: true,
        });
        assert!(!format!("{:?}", SupervisorValue::OutputBytes(private)).contains("private-"));
    }

    #[test]
    fn maximum_byte_page_fits_native_transport_and_never_falls_back_to_text() {
        let page = OutputBytesPage {
            stdout: vec![255; 131072],
            stderr: vec![0; 131072],
            next: OutputCursor {
                stdout: 131072,
                stderr: 131072,
            },
            gap: false,
            complete: true,
        };
        let response = SupervisorResponse::Success(SupervisorValue::OutputBytes(page.into()));
        let wire = serde_json::to_vec(&response).unwrap();
        // Base64 expansion stays below the native runner's 2 MiB stream cap.
        assert!(wire.len() < 400000);
        let decoded: SupervisorResponse = serde_json::from_slice(&wire).unwrap();
        assert_eq!(
            decoded
                .into_result()
                .unwrap()
                .output_bytes()
                .unwrap()
                .stdout
                .len(),
            131072
        );
        let text = SupervisorValue::Output(OutputPage {
            stdout: "lossy text".into(),
            stderr: String::new(),
            next: OutputCursor::default(),
            gap: false,
            complete: true,
        });
        assert!(matches!(text.output_bytes(), Err(Error::State(_))));
    }
}

#[cfg(test)]
mod unknown_operation_tests {
    use super::*;

    /// Requests a client built with `portability` sends, exactly as it encodes
    /// them (pinned by `portability_requests_keep_this_wire_shape`). A supervisor
    /// built without the feature must refuse each one as Unsupported.
    const PORTABILITY_REQUESTS: [&str; 6] = [
        r#"{"operation":"list_directory","parameters":{"path":"docs","limit":256,"cursor":null}}"#,
        r#"{"operation":"read_versioned_file","parameters":{"path":"docs/a","offset":0,"limit":16,"expected":null}}"#,
        r#"{"operation":"publish_file","parameters":{"path":"docs/a","bytes":[1,2],"expected":"Missing","executable":false}}"#,
        r#"{"operation":"export_tree","parameters":{"transfer":{"id":"transfer"}}}"#,
        r#"{"operation":"import_tree","parameters":{"transfer":{"id":"transfer"}}}"#,
        r#"{"operation":"remove_transfer","parameters":{"transfer":{"id":"transfer"}}}"#,
    ];

    fn context() -> SupervisorContext {
        SupervisorContext {
            workspace_id: WorkspaceId::new("workspace").unwrap(),
            root: "/project".into(),
            control: "/control".into(),
            ownership: Ownership::Attached,
        }
    }
    fn request(operation: &str) -> Vec<u8> {
        let operation: serde_json::Value = serde_json::from_str(operation).unwrap();
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "context": context(),
            "operation": operation,
        }))
        .unwrap()
    }
    fn refusal(bytes: &[u8]) -> Error {
        match SupervisorRequest::decode(bytes) {
            Ok(_) => panic!("request decoded"),
            Err(error) => error,
        }
    }

    #[test]
    fn an_operation_this_build_lacks_is_unsupported_whatever_its_parameters() {
        for operation in [
            r#"{"operation":"future_operation","parameters":{"path":"a","nested":[1]}}"#,
            r#"{"operation":"future_operation","parameters":null}"#,
            r#"{"operation":"future_operation"}"#,
        ] {
            let error = refusal(&request(operation));
            assert!(
                matches!(&error, Error::Unsupported(message) if message.contains("`future_operation`")),
                "{error:?}"
            );
        }
        // An implausible name is not echoed back.
        let error = refusal(&request(r#"{"operation":"Secret Value","parameters":{}}"#));
        assert!(matches!(&error, Error::Unsupported(message) if !message.contains("Secret")));
        // The decoded Unknown variant can never be sent.
        assert!(serde_json::to_vec(&SupervisorOperation::Unknown).is_err());
    }

    #[test]
    fn a_known_operation_with_bad_parameters_or_a_non_request_is_invalid_input() {
        for bytes in [
            request(r#"{"operation":"read_file","parameters":{"path":"a"}}"#),
            request(r#"{"operation":"ready","parameters":{"unexpected":true}}"#),
            request(r#"{"operation":7}"#),
            request(r#"{"parameters":{}}"#),
            b"not json".to_vec(),
            br#"{"version":1,"operation":{"operation":"read_file"}}"#.to_vec(),
        ] {
            assert!(
                matches!(refusal(&bytes), Error::InvalidInput(_)),
                "{}",
                String::from_utf8_lossy(&bytes)
            );
        }
        let ready = SupervisorRequest::decode(&request(r#"{"operation":"ready"}"#)).unwrap();
        assert!(matches!(ready.operation, SupervisorOperation::Ready));
    }

    /// The old/new mismatch: a client built with `portability` talking to a
    /// supervisor built without it gets Unsupported for every such operation.
    #[cfg(not(feature = "portability"))]
    #[test]
    fn portability_requests_are_unsupported_by_a_supervisor_without_the_feature() {
        for operation in PORTABILITY_REQUESTS {
            assert!(
                matches!(refusal(&request(operation)), Error::Unsupported(_)),
                "{operation}"
            );
        }
    }

    /// With the feature, the same requests decode, and the client encodes them
    /// exactly as the cross-build test above assumes.
    #[cfg(feature = "portability")]
    #[test]
    fn portability_requests_keep_this_wire_shape() {
        use crate::{FilePrecondition, ListDirectory, PublishFile, ReadVersionedFile};
        let path = |value: &str| WorkspacePath::new(value).unwrap();
        let transfer = || TreeTransfer {
            id: crate::OperationId::new("transfer").unwrap(),
        };
        let operations = [
            SupervisorOperation::ListDirectory(ListDirectory {
                path: path("docs"),
                limit: 256,
                cursor: None,
            }),
            SupervisorOperation::ReadVersionedFile(ReadVersionedFile {
                path: path("docs/a"),
                offset: 0,
                limit: 16,
                expected: None,
            }),
            SupervisorOperation::PublishFile(PublishFile {
                path: path("docs/a"),
                bytes: vec![1, 2],
                expected: FilePrecondition::Missing,
                executable: false,
            }),
            SupervisorOperation::ExportTree {
                transfer: transfer(),
            },
            SupervisorOperation::ImportTree {
                transfer: transfer(),
            },
            SupervisorOperation::RemoveTransfer {
                transfer: transfer(),
            },
        ];
        for (operation, pinned) in operations.into_iter().zip(PORTABILITY_REQUESTS) {
            let encoded =
                serde_json::to_vec(&SupervisorRequest::new(context(), operation)).unwrap();
            let encoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            let pinned: serde_json::Value = serde_json::from_slice(&request(pinned)).unwrap();
            assert_eq!(encoded, pinned);
            assert!(SupervisorRequest::decode(&request(&pinned["operation"].to_string())).is_ok());
        }
    }
}
