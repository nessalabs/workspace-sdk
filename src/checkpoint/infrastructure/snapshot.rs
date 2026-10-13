use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::str::from_utf8;
use tempfile::tempdir_in;
use uuid::Uuid;

impl GitCheckpointEngine {
    pub(super) fn load_checkpoint(&self, reference: &CheckpointRef) -> Result<Checkpoint> {
        let checkpoint: Checkpoint =
            self.read_json(&self.checkpoint_dir(reference)?.join("manifest.json"))?;
        if &checkpoint.manifest.reference != reference {
            return Err(Error::State("checkpoint reference mismatch".into()));
        }
        self.validate_manifest(&checkpoint.manifest)?;
        Ok(checkpoint)
    }

    pub(super) fn validate_manifest(&self, manifest: &CheckpointManifest) -> Result<()> {
        if manifest.version != CHECKPOINT_FORMAT_VERSION {
            return Err(Error::Unsupported(
                "unknown checkpoint format version".into(),
            ));
        }
        valid_id(&manifest.reference.0)?;
        if let Some(parent) = &manifest.parent {
            valid_id(&parent.0)?;
        }
        if manifest.commit.len() != 40 || !manifest.commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid snapshot commit".into()));
        }
        check_entries(&manifest.entries, &self.limits)?;
        let mut sorted = manifest.entries.clone();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        if sorted != manifest.entries
            || fingerprint(&manifest.entries)? != manifest.tree_fingerprint
        {
            return Err(Error::InvalidInput(
                "manifest tree fingerprint mismatch".into(),
            ));
        }
        if manifest.total_bytes != manifest.entries.iter().map(|e| e.size).sum::<u64>()
            || manifest.bundle_bytes > self.limits.max_bundle_bytes
        {
            return Err(Error::InvalidInput("manifest sizes are invalid".into()));
        }
        if serde_json::to_vec(manifest)?.len() as u64 > self.limits.max_manifest_bytes {
            return Err(Error::InvalidInput("manifest exceeds size limit".into()));
        }
        Ok(())
    }

    pub(super) fn save_tree(&self, tree: &Tree, options: CaptureOptions) -> Result<Checkpoint> {
        check_entries(&tree.entries(), &self.limits)?;
        validate_tree_symlinks(tree)?;
        let reference = CheckpointRef(Uuid::new_v4().to_string());
        let temporary = tempdir_in(self.state_directory.join("checkpoints"))?;
        let repo = temporary.path().join("repo");
        self.init_repo(&repo)?;
        let commit = self.commit_tree(&repo, tree, None)?;
        self.git(
            Some(&repo),
            ["update-ref", "refs/heads/snapshot", &commit],
            None,
        )?;
        let bundle = temporary.path().join("snapshot.bundle");
        self.git(
            Some(&repo),
            [
                OsStr::new("bundle"),
                OsStr::new("create"),
                bundle.as_os_str(),
                OsStr::new("refs/heads/snapshot"),
            ],
            None,
        )?;
        let bytes = read_regular(&bundle, self.limits.max_bundle_bytes)?;
        let checkpoint = Checkpoint {
            manifest: CheckpointManifest {
                version: CHECKPOINT_FORMAT_VERSION,
                reference: reference.clone(),
                parent: options.parent,
                environment_reference: options.environment_reference,
                commit,
                tree_fingerprint: tree.fingerprint()?,
                bundle_sha256: digest(&bytes),
                bundle_bytes: bytes.len() as u64,
                total_bytes: tree.total_bytes(),
                entries: tree.entries(),
                excluded: tree.excluded.clone(),
            },
        };
        self.write_json(&temporary.path().join("manifest.json"), &checkpoint)?;
        fs::rename(temporary.path(), self.checkpoint_dir(&reference)?)?;
        Ok(checkpoint)
    }

    pub(super) fn tree_from_repo(&self, repo: &Path, checkpoint: &Checkpoint) -> Result<Tree> {
        let commit = &checkpoint.manifest.commit;
        let output = self.git(Some(repo), ["ls-tree", "-rz", "--full-tree", commit], None)?;
        if output.len() as u64 > self.limits.max_manifest_bytes {
            return Err(Error::InvalidInput(
                "Git tree metadata exceeds size limit".into(),
            ));
        }
        let expected: BTreeMap<_, _> = checkpoint
            .manifest
            .entries
            .iter()
            .map(|e| (e.path.clone(), e))
            .collect();
        let mut files = BTreeMap::new();
        for record in output
            .split(|b| *b == 0)
            .filter(|record| !record.is_empty())
        {
            let record = from_utf8(record)
                .map_err(|_| Error::InvalidInput("non-UTF-8 bundle path".into()))?;
            let (header, path) = record
                .split_once('\t')
                .ok_or_else(|| Error::InvalidInput("invalid Git tree record".into()))?;
            let fields: Vec<_> = header.split(' ').collect();
            if fields.len() != 3 || fields[1] != "blob" {
                return Err(Error::InvalidInput(
                    "bundle contains unsupported object".into(),
                ));
            }
            let entry = expected.get(path).ok_or_else(|| {
                Error::InvalidInput(format!("bundle contains undeclared path: {path}"))
            })?;
            if fields[0].parse::<u32>().ok() != Some(entry.mode) {
                return Err(Error::InvalidInput("bundle file mode mismatch".into()));
            }
            let size = self
                .git_text(repo, ["cat-file", "-s", fields[2]], None)?
                .parse::<u64>()
                .map_err(|_| Error::State("invalid Git object size".into()))?;
            if size != entry.size || size > self.limits.max_file_bytes {
                return Err(Error::InvalidInput("bundle file size mismatch".into()));
            }
            let bytes = self.git(Some(repo), ["cat-file", "blob", fields[2]], None)?;
            if digest(&bytes) != entry.sha256 {
                return Err(Error::InvalidInput("bundle file checksum mismatch".into()));
            }
            if entry.kind == EntryKind::Symlink {
                validate_symlink(path, &bytes)?;
            }
            files.insert(
                path.to_owned(),
                TreeFile {
                    entry: (*entry).clone(),
                    bytes,
                },
            );
        }
        if files.len() != expected.len() {
            return Err(Error::InvalidInput(
                "bundle is missing declared files".into(),
            ));
        }
        Ok(Tree {
            files,
            excluded: checkpoint.manifest.excluded.clone(),
        })
    }

    pub(super) fn load_tree(&self, reference: &CheckpointRef) -> Result<Tree> {
        let checkpoint = self.load_checkpoint(reference)?;
        self.tree_from_repo(&self.checkpoint_dir(reference)?.join("repo"), &checkpoint)
    }
}
