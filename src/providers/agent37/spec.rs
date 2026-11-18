//! What a machine is made from.
use super::failure::Agent37Failure;
use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};

/// The shortest and longest idle time before an instance sleeps.
pub const IDLE: std::ops::RangeInclusive<Duration> =
    Duration::from_secs(300)..=Duration::from_secs(86_400);

/// An instance's shape, as Agent37 sells it. Omitted, the smallest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Size {
    /// vCPUs.
    pub cpu: u8,
    /// Memory, in GB.
    pub memory: u16,
    /// Disk, in GB.
    pub disk: u16,
}

/// What every machine for one purpose is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineSpec {
    pub(super) template: String,
    pub(super) region: String,
    pub(super) size: Option<Size>,
    pub(super) idle: Duration,
    pub(super) variables: BTreeMap<String, String>,
}
impl MachineSpec {
    /// Machines of `template`, pinned to a release (`name@revision`, or a
    /// system template's `name@tag`): every machine made from one spec runs
    /// the same image. They sleep after `idle` with nothing moving through
    /// them, and hold `variables` in their environment.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] for an unpinned or malformed
    /// template, an idle time outside [`IDLE`], or a variable whose name
    /// is not `[A-Z][A-Z0-9_]*`.
    pub fn new(
        template: impl Into<String>,
        idle: Duration,
        variables: BTreeMap<String, String>,
    ) -> Result<Self, Agent37Failure> {
        let template = template.into();
        let pinned = template.split_once('@').is_some_and(|(name, pin)| {
            !name.is_empty()
                && !pin.is_empty()
                && template.len() <= 128
                && template
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
                && template.matches('@').count() == 1
        });
        let named = variables.keys().all(|name| {
            name.len() <= 128
                && name.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        });
        if !pinned || !IDLE.contains(&idle) || !named {
            return Err(Agent37Failure::InvalidInput);
        }
        Ok(Self {
            template,
            region: "us".into(),
            size: None,
            idle,
            variables,
        })
    }
    /// The same machines in another shape.
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }
    /// The pinned template.
    pub fn template(&self) -> &str {
        &self.template
    }
    /// The template's name, without its pin.
    pub(super) fn template_name(&self) -> &str {
        self.template
            .split_once('@')
            .map_or(self.template.as_str(), |(name, _)| name)
    }
}
