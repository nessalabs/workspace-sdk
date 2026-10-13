//! Domain values and lifecycle snapshots for the synchronization bounded context.
//!
//! A checkpoint is an immutable artifact, not a mutable entity. Its reference is
//! a value identifying that artifact. Reconciliation records are snapshots of a
//! review lifecycle managed by the checkpoint service; mutating a returned Rust
//! record does not update that lifecycle. Concept modules group related values
//! and expose the same types through this module's reexports.
//!
//! [`Checkpoint`] describes selected immutable files, while [`Reconciliation`]
//! preserves baseline/local/remote versions and an editable review result.
//! [`ApplyOptions`] makes destination writes explicit; [`ApplyState`] and
//! [`ApplyReceipt`] let applications recover interrupted operations without
//! guessing whether a workload or file publication completed.
//!
//! These types carry values and serializable records, not provider clients or
//! credentials. Engines validate externally supplied records before use. Keep
//! engine state outside the project, retain it while reviewing or recovering,
//! and establish writer quiescence before capture or apply; a fingerprint cannot
//! lock an editor. [`TreeArchive`] is the native target transport envelope, while
//! [`CheckpointBundle`] transfers a standalone Git snapshot between engines.

pub mod apply;
pub mod capture;
pub mod checkpoint;
pub mod file_entry;
pub mod reconciliation;
pub mod restore;
pub mod tree_archive;

pub(crate) use apply::RecoveryJournal;
pub use apply::{ApplyOptions, ApplyReceipt, ApplyState, RecoveryRef};
pub use capture::{CaptureOptions, CheckpointLimits};
pub use checkpoint::{
    Checkpoint, CheckpointBundle, CheckpointManifest, CheckpointRef, CHECKPOINT_FORMAT_VERSION,
};
pub use file_entry::{EntryKind, ExcludedPath, FileEntry};
pub use reconciliation::{
    ConflictRecord, ConflictSide, Reconciliation, ReconciliationRef, ReconciliationStatus,
};
pub use restore::{RestoreReceipt, RestoreTreeReceipt};
pub use tree_archive::{TreeArchive, TreeArchiveEntry};
