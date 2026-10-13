//! Inspect compressed object bounds before handing an untrusted bundle to Git.
//! Git verifies pack integrity and object relationships after this preflight.

use crate::checkpoint::domain::{CheckpointLimits, CheckpointManifest};
use crate::{Error, Result};
use flate2::bufread::ZlibDecoder;
use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::Path;

fn invalid(message: &str) -> Error {
    Error::InvalidInput(format!("invalid/bounded Git pack: {message}"))
}

fn byte(bytes: &[u8], position: &mut usize) -> Result<u8> {
    let value = bytes
        .get(*position)
        .copied()
        .ok_or_else(|| invalid("truncated header"))?;
    *position += 1;
    Ok(value)
}

fn delta_integer(bytes: &[u8], position: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let part = byte(bytes, position)?;
        if shift >= 63 {
            return Err(invalid("delta size overflow"));
        }
        value |= u64::from(part & 0x7f) << shift;
        if part & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

pub(crate) fn preflight_pack(
    bytes: &[u8],
    manifest: &CheckpointManifest,
    limits: &CheckpointLimits,
) -> Result<()> {
    if bytes.len() < 32 || &bytes[..4] != b"PACK" {
        return Err(invalid("missing pack header"));
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    if !matches!(version, 2 | 3) {
        return Err(invalid("unsupported pack version"));
    }
    let count = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut directories = BTreeSet::new();
    for entry in &manifest.entries {
        let mut parent = Path::new(&entry.path).parent();
        while let Some(path) = parent {
            directories.insert(path.to_owned());
            parent = path.parent();
        }
    }
    // At most one blob per declared file, one tree per directory plus root,
    // and one commit. Git later rejects unreachable/extraneous objects.
    if count
        > manifest
            .entries
            .len()
            .saturating_add(directories.len())
            .saturating_add(2)
    {
        return Err(invalid("too many objects for the declared tree"));
    }
    let mut position = 12;
    let mut expanded_total = 0u64;
    let cap = limits
        .max_total_bytes
        .saturating_add(limits.max_manifest_bytes);
    for _ in 0..count {
        let first = byte(bytes, &mut position)?;
        let kind = (first >> 4) & 7;
        let mut size = u64::from(first & 15);
        let mut continuation = first & 0x80 != 0;
        let mut shift = 4;
        while continuation {
            let next = byte(bytes, &mut position)?;
            if shift >= 63 {
                return Err(invalid("object size overflow"));
            }
            size |= u64::from(next & 0x7f) << shift;
            continuation = next & 0x80 != 0;
            shift += 7;
        }
        let object_limit = match kind {
            1 | 2 => limits.max_manifest_bytes,
            3 => limits.max_file_bytes,
            6 | 7 => limits
                .max_file_bytes
                .saturating_add(limits.max_manifest_bytes),
            _ => return Err(invalid("unsupported object type")),
        };
        if size > object_limit {
            return Err(invalid("object exceeds expanded size limit"));
        }
        if kind == 6 {
            let mut traversed = 0;
            loop {
                let value = byte(bytes, &mut position)?;
                traversed += 1;
                if traversed > 10 {
                    return Err(invalid("delta offset overflow"));
                }
                if value & 0x80 == 0 {
                    break;
                }
            }
        } else if kind == 7 {
            position = position
                .checked_add(20)
                .ok_or_else(|| invalid("offset overflow"))?;
        }
        let remaining = bytes
            .get(position..)
            .ok_or_else(|| invalid("truncated object"))?;
        let mut decoder = ZlibDecoder::new(remaining);
        let expanded = if matches!(kind, 6 | 7) {
            let mut instructions = Vec::new();
            (&mut decoder)
                .take(size.saturating_add(1))
                .read_to_end(&mut instructions)?;
            if instructions.len() as u64 != size {
                return Err(invalid("delta data length mismatch"));
            }
            let mut delta_position = 0;
            let base_size = delta_integer(&instructions, &mut delta_position)?;
            let result_size = delta_integer(&instructions, &mut delta_position)?;
            if base_size > object_limit || result_size > object_limit {
                return Err(invalid("expanded delta exceeds size limit"));
            }
            result_size
        } else {
            let read = io::copy(
                &mut (&mut decoder).take(size.saturating_add(1)),
                &mut io::sink(),
            )?;
            if read != size {
                return Err(invalid("object data length mismatch"));
            }
            size
        };
        let consumed =
            usize::try_from(decoder.total_in()).map_err(|_| invalid("compressed size overflow"))?;
        if consumed == 0 {
            return Err(invalid("empty compressed object"));
        }
        position = position
            .checked_add(consumed)
            .ok_or_else(|| invalid("pack offset overflow"))?;
        expanded_total = expanded_total
            .checked_add(expanded)
            .ok_or_else(|| invalid("expanded size overflow"))?;
        if expanded_total > cap {
            return Err(invalid("expanded pack exceeds total size limit"));
        }
    }
    if position.checked_add(20) != Some(bytes.len()) {
        return Err(invalid("pack has trailing data or truncated checksum"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::{CheckpointRef, EntryKind, FileEntry};

    #[test]
    fn declared_decompression_bomb_is_rejected_before_git_is_invoked() {
        let mut bytes = b"PACK\0\0\0\x02\0\0\0\x01".to_vec();
        let mut size = 1024u64 * 1024 * 1024;
        let mut first = (3 << 4) | (size as u8 & 15);
        size >>= 4;
        if size != 0 {
            first |= 128;
        }
        bytes.push(first);
        while size != 0 {
            let mut part = size as u8 & 127;
            size >>= 7;
            if size != 0 {
                part |= 128;
            }
            bytes.push(part);
        }
        bytes.extend([0; 20]);
        let manifest = CheckpointManifest {
            version: 1,
            reference: CheckpointRef("test".into()),
            parent: None,
            environment_reference: None,
            commit: "0".repeat(40),
            tree_fingerprint: "0".repeat(64),
            bundle_sha256: "0".repeat(64),
            bundle_bytes: bytes.len() as u64,
            total_bytes: 1,
            entries: vec![FileEntry {
                path: "file".into(),
                kind: EntryKind::File,
                mode: 100644,
                size: 1,
                sha256: "0".repeat(64),
            }],
            excluded: vec![],
        };
        let error = preflight_pack(&bytes, &manifest, &CheckpointLimits::default()).unwrap_err();
        assert!(error.to_string().contains("expanded size limit"));
    }
}
