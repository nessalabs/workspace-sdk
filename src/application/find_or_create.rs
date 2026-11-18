//! One creation algorithm for every cloud: find what an earlier attempt
//! with the same key made, or make it, and never make a second.
//!
//! ```text
//! Native:  submit(key) ─► Named ─► Answered
//!                      └► Unknown ─► Uncertain (ask again with the key)
//! Tagged:  find(key) ─► one ─► FoundBefore
//!                   ├► two or more ─► Ambiguous
//!                   └► none ─► submit(key) ─► Named ─► Answered
//!                                         └► Unknown ─► find(key)
//!                                              one ─► FoundAfterLostAnswer
//!                                              none or failed ─► Uncertain
//!                                              two or more ─► Ambiguous
//! ```
//!
//! A tagged cloud holds only while attempts for one key are serialized: two
//! attempts that both find nothing both submit. Within one process the
//! SDK's per-workspace record serializes them; across processes, the caller
//! must (one owner per account, or a lock of its own). Two targets carrying
//! one key are then reported, never chosen between.
use crate::ports::{ICreationSite, Idempotency, Submitted};

/// How the target a creation returned came to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationOrigin {
    /// The cloud named it in answer to this attempt's request. On a
    /// [`Idempotency::Native`] cloud it may be an earlier attempt's, replayed.
    Answered,
    /// An earlier attempt made it; found before asking.
    FoundBefore,
    /// This attempt's request lost its answer; found after it, so this
    /// attempt (or one serialized with it) made it.
    FoundAfterLostAnswer,
}

/// Why no target was returned.
#[derive(Debug, PartialEq, Eq)]
pub enum CreationFailure<E> {
    /// A target may or may not have been made: ask again with the same key.
    Uncertain,
    /// More than one target carries the key.
    Ambiguous,
    /// The cloud failed before anything was asked to be made.
    Site(E),
}

/// The target `site` makes under `key`: one an earlier attempt made, or a
/// new one.
///
/// # Errors
/// See [`CreationFailure`]. A failure of the request to make one is
/// [`CreationFailure::Site`] only when the site says so with an error: an
/// unknown outcome is always [`CreationFailure::Uncertain`] or resolved by
/// finding.
pub async fn find_or_create<S: ICreationSite>(
    site: &S,
    key: &str,
) -> Result<(S::Target, CreationOrigin), CreationFailure<S::Error>> {
    let tagged = site.idempotency() == Idempotency::Tagged;
    if tagged {
        if let Some(found) = one(site.find(key).await.map_err(CreationFailure::Site)?)? {
            return Ok((found, CreationOrigin::FoundBefore));
        }
    }
    match site.submit(key).await.map_err(CreationFailure::Site)? {
        Submitted::Named(target) => Ok((target, CreationOrigin::Answered)),
        Submitted::Unknown if tagged => match site.find(key).await {
            Ok(found) => match one(found)? {
                Some(found) => Ok((found, CreationOrigin::FoundAfterLostAnswer)),
                None => Err(CreationFailure::Uncertain),
            },
            Err(_) => Err(CreationFailure::Uncertain),
        },
        Submitted::Unknown => Err(CreationFailure::Uncertain),
    }
}

/// The one target in `found`, if there is exactly one.
fn one<T, E>(found: Vec<T>) -> Result<Option<T>, CreationFailure<E>> {
    let mut found = found.into_iter();
    let first = found.next();
    if found.next().is_some() {
        return Err(CreationFailure::Ambiguous);
    }
    Ok(first)
}

#[cfg(test)]
#[path = "find_or_create_tests.rs"]
mod tests;
