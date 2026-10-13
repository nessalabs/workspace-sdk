use crate::checkpoint::domain::*;
use crate::Result;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) struct TreeFile {
    pub entry: FileEntry,
    pub bytes: Vec<u8>,
}

pub(crate) struct Tree {
    pub files: BTreeMap<String, TreeFile>,
    pub excluded: Vec<ExcludedPath>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn fingerprint(entries: &[FileEntry]) -> Result<String> {
    Ok(digest(&serde_json::to_vec(entries)?))
}

impl Tree {
    pub fn entries(&self) -> Vec<FileEntry> {
        self.files.values().map(|f| f.entry.clone()).collect()
    }
    pub fn fingerprint(&self) -> Result<String> {
        fingerprint(&self.entries())
    }
    pub fn total_bytes(&self) -> u64 {
        self.files.values().map(|f| f.entry.size).sum()
    }
}
