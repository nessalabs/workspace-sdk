//! What a machine is made from.
use super::failure::Agent37Failure;
use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};

/// The shortest and longest idle time before an instance sleeps.
pub const IDLE: std::ops::RangeInclusive<Duration> =
    Duration::from_secs(300)..=Duration::from_secs(86_400);

/// The shapes a `default` instance comes in: vCPUs, memory in GB, and the
/// disk range in GB. Agent37 answers any other combination with
/// `invalid_request`, or quietly gives a request for more cores the
/// shape's own cores.
const SHAPES: [(u8, u16, std::ops::RangeInclusive<u16>); 4] = [
    (2, 4, 2..=12),
    (4, 8, 2..=20),
    (4, 16, 2..=40),
    (4, 32, 2..=80),
];

/// An instance's shape, one of those Agent37 sells. Omitted, the smallest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Size {
    cpu: u8,
    memory: u16,
    disk: u16,
}
impl Size {
    /// `cpu` vCPUs, `memory` GB of memory and `disk` GB of disk.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] unless `cpu` and `memory` name a
    /// shape Agent37 sells and `disk` is within that shape's range.
    pub fn new(cpu: u8, memory: u16, disk: u16) -> Result<Self, Agent37Failure> {
        SHAPES
            .iter()
            .any(|(c, m, disks)| *c == cpu && *m == memory && disks.contains(&disk))
            .then_some(Self { cpu, memory, disk })
            .ok_or(Agent37Failure::InvalidInput)
    }
}

/// Where machines run. The machines here are Agent37's `default`
/// instances, which run only in `us`: `eu` and `ap-south` take VM and
/// performance instances alone, at another price, so they are not offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Region {
    /// The United States.
    #[default]
    Us,
}
impl Region {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Us => "us",
        }
    }
}

/// What every machine for one purpose is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineSpec {
    pub(super) template: String,
    pub(super) region: Region,
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
            region: Region::Us,
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
    /// The same machines in another region.
    pub fn in_region(mut self, region: Region) -> Self {
        self.region = region;
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
