//! Resource use of model workers, measured from the OS (Settings →
//! Performance). Nothing here is estimated: when a number can't be read,
//! the answer is `None` and the UI says "not measured".

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Resident memory (working set) of a process, in MiB.
pub fn process_memory_mb(pid: u32) -> Option<u64> {
    imp::working_set_bytes(pid).map(|b| b / (1024 * 1024))
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    pub fn working_set_bytes(pid: u32) -> Option<u64> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let mut c = PROCESS_MEMORY_COUNTERS {
                cb,
                ..Default::default()
            };
            let ok = GetProcessMemoryInfo(h, &mut c, cb).is_ok();
            let _ = CloseHandle(h);
            ok.then_some(c.WorkingSetSize as u64)
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn working_set_bytes(pid: u32) -> Option<u64> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        super::parse_vmrss_kb(&s).map(|kb| kb * 1024)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;
    use std::time::Duration;

    pub fn working_set_bytes(pid: u32) -> Option<u64> {
        let mut cmd = Command::new("ps");
        cmd.args(["-o", "rss=", "-p", &pid.to_string()]);
        let out = super::run_with_timeout(cmd, Duration::from_secs(3))?;
        super::parse_ps_rss_kb(&String::from_utf8_lossy(&out)).map(|kb| kb * 1024)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn working_set_bytes(_pid: u32) -> Option<u64> {
        None
    }
}

/// Parse `ps -o rss= -p <pid>` output (resident memory in KiB).
pub fn parse_ps_rss_kb(out: &str) -> Option<u64> {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .parse()
        .ok()
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<Vec<u8>> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().ok()?;
                return status.success().then_some(out.stdout);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[allow(dead_code)]
fn parse_vmrss_kb(status: &str) -> Option<u64> {
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GpuMemory {
    pub name: String,
    /// Used by all programs on this GPU, MiB.
    pub used_mb: u64,
    pub total_mb: u64,
}

/// `nvidia-smi --query-gpu=name,memory.used,memory.total
/// --format=csv,noheader,nounits` output → first GPU.
pub fn parse_nvidia_smi(out: &str) -> Option<GpuMemory> {
    let line = out.lines().find(|l| !l.trim().is_empty())?;
    let mut f = line.split(',').map(str::trim);
    let name = f.next()?.to_string();
    let used_mb = f.next()?.parse().ok()?;
    let total_mb = f.next()?.parse().ok()?;
    (!name.is_empty()).then_some(GpuMemory {
        name,
        used_mb,
        total_mb,
    })
}

/// GPU memory in use, from `nvidia-smi` when the NVIDIA driver provides it.
/// Other GPUs (and Apple Silicon unified memory): `None`. Never waits more than ~3 s.
pub fn gpu_memory() -> Option<GpuMemory> {
    #[cfg(target_os = "macos")]
    {
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut c = Command::new("nvidia-smi");
        c.args([
            "--query-gpu=name,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ]);
        let out = run_with_timeout(c, Duration::from_secs(3))?;
        parse_nvidia_smi(&String::from_utf8_lossy(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_and_rejects_noise() {
        assert_eq!(
            parse_nvidia_smi("NVIDIA GeForce RTX 4080, 2154, 16376\n"),
            Some(GpuMemory {
                name: "NVIDIA GeForce RTX 4080".into(),
                used_mb: 2154,
                total_mb: 16376
            })
        );
        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("x, [N/A], 1"), None);
    }

    #[test]
    fn vmrss_line() {
        assert_eq!(parse_vmrss_kb("Name: a\nVmRSS:\t  2048 kB\n"), Some(2048));
        assert_eq!(parse_vmrss_kb("Name: a"), None);
    }

    #[test]
    fn ps_rss_line() {
        assert_eq!(parse_ps_rss_kb("  49152 \n"), Some(49152));
        assert_eq!(parse_ps_rss_kb("\n"), None);
        assert_eq!(parse_ps_rss_kb("RSS\n"), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn own_process_has_memory() {
        assert!(process_memory_mb(std::process::id()).is_some());
    }
}
