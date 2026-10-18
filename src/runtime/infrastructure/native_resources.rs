use crate::domain::{HostGpu, HostResources};
use crate::Result;
use std::env::consts::{ARCH, OS};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::{available_parallelism, sleep};
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
use std::{mem::size_of, ptr::null_mut};

const MIB: u64 = 1024 * 1024;

/// Inspect host-visible capacities without provisioning or allocating resources.
pub(crate) fn inspect_resources(workspace_root: &Path) -> Result<HostResources> {
    let mut cpus = available_parallelism()
        .ok()
        .and_then(|value| u32::try_from(value.get()).ok());
    if let Some(quota) = cpu_quota() {
        cpus = Some(cpus.map_or(quota, |current| current.min(quota)));
    }
    let mut memory = physical_memory();
    for limit in [
        "/sys/fs/cgroup/memory.max",
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
    ] {
        if let Some(limit) = read_integer(Path::new(limit)).filter(|value| *value > 0) {
            memory = Some(memory.map_or(limit, |current| current.min(limit)));
        }
    }
    Ok(HostResources {
        os: os_name().into(),
        architecture: architecture_name().into(),
        cpus,
        memory_mib: memory.map(|bytes| bytes / MIB),
        disk_mib: Some(fs2::available_space(workspace_root)? / MIB),
        gpus: inspect_gpus(),
    })
}

fn os_name() -> &'static str {
    match OS {
        "macos" => "darwin",
        other => other,
    }
}

fn architecture_name() -> &'static str {
    match (OS, ARCH) {
        ("macos", "aarch64") => "arm64",
        (_, architecture) => architecture,
    }
}

fn cpu_quota() -> Option<u32> {
    let value = fs::read_to_string("/sys/fs/cgroup/cpu.max").ok()?;
    let mut parts = value.split_whitespace();
    let quota: u64 = parts.next()?.parse().ok()?;
    let period: u64 = parts.next()?.parse().ok()?;
    u32::try_from(quota.checked_div(period)?.max(1)).ok()
}

fn read_integer(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn physical_memory() -> Option<u64> {
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || page_size <= 0 {
        return None;
    }
    (pages as u64).checked_mul(page_size as u64)
}

#[cfg(target_os = "macos")]
fn physical_memory() -> Option<u64> {
    let mut memory = 0u64;
    let mut size = size_of::<u64>();
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut memory as *mut u64).cast(),
            &mut size,
            null_mut(),
            0,
        )
    };
    (result == 0 && size == size_of::<u64>() && memory > 0).then_some(memory)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn physical_memory() -> Option<u64> {
    None
}

fn inspect_gpus() -> Vec<HostGpu> {
    let Ok(child) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    collect_gpus(child, Duration::from_secs(5))
}

fn collect_gpus(mut child: Child, budget: Duration) -> Vec<HostGpu> {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut output = String::new();
                if child
                    .stdout
                    .take()
                    .is_some_and(|stream| stream.take(65_537).read_to_string(&mut output).is_ok())
                    && output.len() <= 65_536
                {
                    return parse_gpus(&output);
                }
                return Vec::new();
            }
            Ok(Some(_)) => return Vec::new(),
            Ok(None) if Instant::now() < deadline => {
                sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
        }
    }
}

fn parse_gpus(output: &str) -> Vec<HostGpu> {
    output
        .lines()
        .filter_map(|line| {
            let (model, memory) = line.rsplit_once(',')?;
            Some(HostGpu {
                model: model.trim().to_owned(),
                memory_mib: memory.trim().parse().ok()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_capacity_is_per_device_and_malformed_rows_are_not_guessed() {
        let actual = parse_gpus("GPU A, 24576\nnot a device\nGPU B, unknown\nGPU C, 49152\n");
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0].model, "GPU A");
        assert_eq!(actual[0].memory_mib, 24_576);
        assert_eq!(actual[1].memory_mib, 49_152);
    }
}

#[cfg(test)]
#[path = "native_resources_tests.rs"]
mod collection_tests;
