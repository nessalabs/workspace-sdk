//! Typed wire contract between provider transports and the Rust supervisor.
//!
//! Request values are serialized only when crossing a process/API boundary.
//! Credentials are transient fields, never part of persisted run records.
use crate::{
    CommandSpec, Error, HostResources, InputOutcome, OutputCursor, OutputPage, Ownership, Result,
    RunId, RunState, SecretEnvironment, SecretInput, TerminalSize, TreeExportReceipt,
    TreeImportReceipt, TreeTransfer, WorkspaceId, WorkspacePath,
};
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
    ListDirectory(crate::ListDirectory),
    ReadVersionedFile(crate::ReadVersionedFile),
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
    StopOwnedRuns,
    DestroyOwnedFiles,
    ExportTree {
        transfer: TreeTransfer,
    },
    ImportTree {
        transfer: TreeTransfer,
    },
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
}
impl SupervisorOperation {
    /// Payload-bearing operations require private provider transport. Exhaustive
    /// classification prevents a new input operation from falling into command history.
    pub(crate) fn requires_private_transport(&self) -> bool {
        match self {
            Self::Start { .. }
            | Self::StartTerminal { .. }
            | Self::TerminalInput { .. }
            | Self::PublishFile(..)
            | Self::WriteFile { .. }
            | Self::WritePrivateFile { .. } => true,
            Self::Initialize { .. }
            | Self::Ready
            | Self::ListDirectory(..)
            | Self::ReadVersionedFile(..)
            | Self::ReadFile { .. }
            | Self::TerminalResize { .. }
            | Self::Inspect { .. }
            | Self::Cancel { .. }
            | Self::Output { .. }
            | Self::StopOwnedRuns
            | Self::DestroyOwnedFiles
            | Self::ExportTree { .. }
            | Self::ImportTree { .. }
            | Self::RemoveTransfer { .. }
            | Self::EnsurePrivateDirectory { .. }
            | Self::InspectResources
            | Self::PrepareRequest { .. }
            | Self::RemoveRequest { .. } => false,
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
}
/// Typed file transfer response. Bytes use base64 only at the serialization boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferredFileChunk {
    #[serde(with = "binary")]
    pub bytes: Vec<u8>,
    pub next_offset: u64,
    pub eof: bool,
}
/// A successful operation result, carrying its concrete domain type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SupervisorValue {
    Empty,
    DirectoryPage(crate::DirectoryPage),
    VersionedFileChunk(crate::VersionedFileChunk),
    PublishOutcome(crate::PublishOutcome),
    /// Target rejection before publication; transport failures remain Unknown.
    PublishRejected(SupervisorFault),
    Run(RunState),
    Input(InputOutcome),
    /// Definitive rejection established before any input could be dispatched.
    InputRejected(SupervisorFault),
    Output(OutputPage),
    FileChunk(TransferredFileChunk),
    TreeExport(TreeExportReceipt),
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
    pub fn file_chunk(self) -> Result<TransferredFileChunk> {
        match self {
            Self::FileChunk(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
    pub fn tree_export(self) -> Result<TreeExportReceipt> {
        match self {
            Self::TreeExport(value) => Ok(value),
            _ => Err(unexpected()),
        }
    }
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
