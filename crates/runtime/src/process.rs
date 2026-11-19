//! Child process supervision shared by every worker.
//!
//! Guarantees (docs/05):
//! - Windows: the child is placed in a Job Object with KILL_ON_JOB_CLOSE, so
//!   it dies with the app even if the app crashes or is killed. No console
//!   window is created.
//! - Linux: PR_SET_PDEATHSIG(SIGKILL). The child is spawned from a dedicated
//!   supervisor thread that lives exactly as long as the child (the signal is
//!   tied to the spawning *thread*).
//! - macOS: an inherited close-on-exec watchdog pipe monitors parent exit and
//!   sends SIGTERM -> SIGKILL if the parent dies uncleanly; clean shutdown kills
//!   the watchdog and terminates the worker synchronously.
//! - All platforms: active worker PID + executable metadata are recorded in app
//!   data (`workers.json`), and startup cleanup reaps any verified stale
//!   Tamgabul worker process whose OS executable name still matches.
//! - stdout/stderr are drained continuously (a full pipe would block the
//!   worker) into a small ring buffer used only for error reports.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::RuntimeError;

const LOG_LINES: usize = 80;
const REGISTRY_FILE: &str = "workers.json";

/// Worker executables Tamgabul is allowed to reap if orphaned after a crash.
pub const DEFAULT_WORKER_EXES: &[&str] = &["llama-server", "whisper-server", "piper"];

static REGISTRY_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

fn registry_slot() -> &'static Mutex<Option<PathBuf>> {
    REGISTRY_DIR.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub pid: u32,
    pub exe_name: String,
    pub started_ms: u64,
}

pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(target_os = "linux")]
fn die_with_parent(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}
#[cfg(not(target_os = "linux"))]
fn die_with_parent(_cmd: &mut Command) {}

#[cfg(target_os = "macos")]
struct MacWatchdog {
    helper: Child,
    _write_pipe: std::fs::File,
}

#[cfg(target_os = "macos")]
impl MacWatchdog {
    fn spawn(target_pid: u32) -> Option<Self> {
        use std::os::unix::io::FromRawFd;
        let mut fds = [-1i32; 2];
        // SAFETY: creating a plain OS pipe with valid 2-int buffer.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: both ends are valid pipe fds returned by pipe(2).
        unsafe {
            libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let read_end = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let write_end = unsafe { std::fs::File::from_raw_fd(fds[1]) };

        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            "cat >/dev/null 2>&1; kill -TERM \"$1\" 2>/dev/null; sleep 1; kill -KILL \"$1\" 2>/dev/null",
            "_",
            &target_pid.to_string(),
        ])
        .stdin(Stdio::from(read_end))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        match cmd.spawn() {
            Ok(helper) => Some(Self {
                helper,
                _write_pipe: write_end,
            }),
            Err(_) => None,
        }
    }

    fn stop(mut self) {
        let _ = self.helper.kill();
        let _ = self.helper.wait();
    }
}

fn exe_stem(path_or_name: &str) -> String {
    let base = Path::new(path_or_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path_or_name);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .to_ascii_lowercase()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_records(path: &Path) -> Vec<WorkerRecord> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Vec<WorkerRecord>>(&bytes) {
        Ok(v) => v,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            Vec::new()
        }
    }
}

fn write_records(path: &Path, records: &[WorkerRecord]) {
    if records.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_vec(records) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Configure where active worker PID metadata (`workers.json`) is stored.
pub fn set_worker_registry_dir(dir: PathBuf) {
    if let Ok(mut guard) = registry_slot().lock() {
        *guard = Some(dir);
    }
}

fn current_registry_path() -> Option<PathBuf> {
    registry_slot()
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|d| d.join(REGISTRY_FILE))
}

pub(crate) fn record_exe_pid(pid: u32, exe: &Path) {
    let Some(path) = current_registry_path() else {
        return;
    };
    record_pid_in(&path, pid, &exe.to_string_lossy());
}

pub(crate) fn unrecord_pid(pid: u32) {
    let Some(path) = current_registry_path() else {
        return;
    };
    unrecord_pid_in(&path, pid);
}

fn record_pid_in(path: &Path, pid: u32, exe_name: &str) {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut records = read_records(path);
    records.retain(|r| r.pid != pid);
    records.push(WorkerRecord {
        pid,
        exe_name: exe_stem(exe_name),
        started_ms: now_ms(),
    });
    write_records(path, &records);
}

fn unrecord_pid_in(path: &Path, pid: u32) {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut records = read_records(path);
    let before = records.len();
    records.retain(|r| r.pid != pid);
    if records.len() != before {
        write_records(path, &records);
    }
}

#[cfg(windows)]
fn query_and_kill_if_matches(pid: u32, expected_stem: &str, allowed_stems: &[String]) -> bool {
    const PROCESS_TERMINATE: u32 = 0x0001;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INFINITE_WAIT_MS: u32 = 5000;

    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            exe_name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn TerminateProcess(process: isize, exit_code: u32) -> i32;
        fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
        fn CloseHandle(handle: isize) -> i32;
    }

    // SAFETY: Win32 handle is checked for NULL and always closed before returning.
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        );
        if handle == 0 {
            return false;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        let killed = if ok != 0 && len > 0 {
            let live_path = String::from_utf16_lossy(&buf[..len as usize]);
            let live_stem = exe_stem(&live_path);
            if live_stem == expected_stem && allowed_stems.iter().any(|a| a == &live_stem) {
                if TerminateProcess(handle, 1) != 0 {
                    let _ = WaitForSingleObject(handle, INFINITE_WAIT_MS);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        let _ = CloseHandle(handle);
        killed
    }
}

#[cfg(target_os = "linux")]
fn live_exe_stem(pid: u32) -> Option<String> {
    if let Ok(link) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return Some(exe_stem(&link.to_string_lossy()));
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| exe_stem(s.trim()))
}

#[cfg(target_os = "macos")]
fn live_exe_stem(pid: u32) -> Option<String> {
    let out = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| exe_stem(&s))
}

#[cfg(unix)]
fn query_and_kill_if_matches(pid: u32, expected_stem: &str, allowed_stems: &[String]) -> bool {
    let Some(live_stem) = live_exe_stem(pid) else {
        return false;
    };
    if live_stem != expected_stem || !allowed_stems.iter().any(|a| a == &live_stem) {
        return false;
    }
    // SAFETY: verified above that `pid` matches an expected stale worker stem.
    unsafe {
        if libc::kill(pid as i32, libc::SIGKILL) == 0 {
            return true;
        }
    }
    false
}

/// Reap stale worker processes recorded by an earlier unclean app exit.
/// Returns the number of verified worker processes terminated.
pub fn cleanup_stale_workers(dir: &Path) -> u32 {
    cleanup_stale_workers_for(dir, DEFAULT_WORKER_EXES)
}

/// Same as [`cleanup_stale_workers`] with an explicit executable stem allowlist.
pub fn cleanup_stale_workers_for(dir: &Path, allowed_exes: &[&str]) -> u32 {
    let path = dir.join(REGISTRY_FILE);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let records = read_records(&path);
    if records.is_empty() {
        return 0;
    }
    let allowed: Vec<String> = allowed_exes.iter().map(|s| exe_stem(s)).collect();
    let self_pid = std::process::id();
    let mut killed = 0u32;
    for r in records {
        if r.pid == 0 || r.pid == self_pid {
            continue;
        }
        let expected = exe_stem(&r.exe_name);
        if !allowed.iter().any(|a| a == &expected) {
            continue;
        }
        if query_and_kill_if_matches(r.pid, &expected, &allowed) {
            killed += 1;
        }
    }
    let _ = std::fs::remove_file(&path);
    killed
}

#[derive(Default)]
struct Shared {
    child: Option<Child>,
    exit: Option<String>,
    #[cfg(target_os = "macos")]
    watchdog: Option<MacWatchdog>,
}

/// A supervised child process.
pub struct Supervised {
    pid: u32,
    shared: Arc<Mutex<Shared>>,
    log: Arc<Mutex<VecDeque<String>>>,
    #[cfg(windows)]
    _job: win32job::Job,
}

fn drain<R: Read + Send + 'static>(r: R, log: Arc<Mutex<VecDeque<String>>>) {
    std::thread::Builder::new()
        .name("tamgabul-worker-log".into())
        .spawn(move || {
            for line in BufReader::new(r).lines() {
                let Ok(line) = line else { break };
                if let Ok(mut l) = log.lock() {
                    if l.len() == LOG_LINES {
                        l.pop_front();
                    }
                    l.push_back(line);
                }
            }
        })
        .ok();
}

impl Supervised {
    /// Spawn `cmd` from a dedicated supervisor thread.
    pub fn spawn(mut cmd: Command) -> Result<Self, RuntimeError> {
        let exe = PathBuf::from(cmd.get_program());
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut cmd);
        die_with_parent(&mut cmd);

        let shared = Arc::new(Mutex::new(Shared::default()));
        let log = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        let (tx, rx) = mpsc::channel::<Result<u32, RuntimeError>>();
        {
            let shared = shared.clone();
            let log = log.clone();
            let exe_for_thread = exe.clone();
            std::thread::Builder::new()
                .name("tamgabul-worker".into())
                .spawn(move || {
                    let mut child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = tx.send(Err(RuntimeError::LoadFailed(format!("spawn: {e}"))));
                            return;
                        }
                    };
                    if let Some(o) = child.stdout.take() {
                        drain(o, log.clone());
                    }
                    if let Some(e) = child.stderr.take() {
                        drain(e, log);
                    }
                    let pid = child.id();
                    record_exe_pid(pid, &exe_for_thread);
                    #[cfg(target_os = "macos")]
                    let watchdog = MacWatchdog::spawn(pid);
                    {
                        let mut s = shared.lock().expect("worker state");
                        s.child = Some(child);
                        #[cfg(target_os = "macos")]
                        {
                            s.watchdog = watchdog;
                        }
                    }
                    let _ = tx.send(Ok(pid));
                    // Live exactly as long as the child (PDEATHSIG is per thread).
                    loop {
                        std::thread::sleep(Duration::from_millis(100));
                        let mut s = match shared.lock() {
                            Ok(s) => s,
                            Err(_) => return,
                        };
                        let Some(c) = s.child.as_mut() else { return };
                        match c.try_wait() {
                            Ok(Some(status)) => {
                                s.exit = Some(status.to_string());
                                s.child = None;
                                #[cfg(target_os = "macos")]
                                if let Some(wd) = s.watchdog.take() {
                                    wd.stop();
                                }
                                unrecord_pid(pid);
                                return;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                s.exit = Some(format!("wait failed: {e}"));
                                s.child = None;
                                #[cfg(target_os = "macos")]
                                if let Some(wd) = s.watchdog.take() {
                                    wd.stop();
                                }
                                unrecord_pid(pid);
                                return;
                            }
                        }
                    }
                })
                .map_err(|e| RuntimeError::LoadFailed(format!("thread: {e}")))?;
        }
        let pid = rx
            .recv()
            .map_err(|_| RuntimeError::LoadFailed("supervisor died".into()))??;

        #[cfg(windows)]
        let job = {
            let mut info = win32job::ExtendedLimitInfo::new();
            info.limit_kill_on_job_close();
            let job = win32job::Job::create_with_limit_info(&info)
                .map_err(|e| RuntimeError::LoadFailed(format!("job object: {e}")))?;
            let assigned = {
                use std::os::windows::io::AsRawHandle;
                let s = shared.lock().expect("worker state");
                s.child
                    .as_ref()
                    .map(|c| job.assign_process(c.as_raw_handle() as isize))
            };
            if let Some(Err(e)) = assigned {
                // Without the job the worker could outlive a crash: refuse it.
                let s = Self {
                    pid,
                    shared: shared.clone(),
                    log: log.clone(),
                    _job: job,
                };
                let _ = s.terminate(Duration::ZERO);
                return Err(RuntimeError::LoadFailed(format!("job assign: {e}")));
            }
            job
        };

        Ok(Self {
            pid,
            shared,
            log,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `Some(reason)` once the process has exited (for any reason).
    pub fn exit_reason(&self) -> Option<String> {
        let mut s = self.shared.lock().ok()?;
        if s.exit.is_some() {
            return s.exit.clone();
        }
        // Check directly too, so callers don't wait for the next poll.
        if let Some(status) = s.child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
            s.exit = Some(status.to_string());
            s.child = None;
            #[cfg(target_os = "macos")]
            if let Some(wd) = s.watchdog.take() {
                wd.stop();
            }
            unrecord_pid(self.pid);
        }
        s.exit.clone()
    }

    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.log
            .lock()
            .map(|l| l.iter().rev().take(n).rev().cloned().collect())
            .unwrap_or_default()
    }

    /// Stop the process and block until the OS reports it gone. `grace`
    /// lets it exit on its own first (SIGTERM on unix); Windows has no
    /// graceful signal for a console-less child, so it is terminated.
    pub fn terminate(&self, grace: Duration) -> Result<(), RuntimeError> {
        #[cfg(target_os = "macos")]
        if let Some(wd) = self.shared.lock().ok().and_then(|mut s| s.watchdog.take()) {
            wd.stop();
        }

        #[cfg(unix)]
        if !grace.is_zero() && self.exit_reason().is_none() {
            // SAFETY: plain kill(2) on a pid we spawned and still own.
            unsafe {
                libc::kill(self.pid as i32, libc::SIGTERM);
            }
            let until = Instant::now() + grace;
            while Instant::now() < until && self.exit_reason().is_none() {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        #[cfg(not(unix))]
        let _ = grace;

        {
            let mut s = self
                .shared
                .lock()
                .map_err(|_| RuntimeError::Io("worker state poisoned".into()))?;
            if let Some(c) = s.child.as_mut() {
                let _ = c.kill();
                let status = c
                    .wait()
                    .map_err(|e| RuntimeError::Io(format!("wait: {e}")))?;
                s.exit = Some(status.to_string());
                s.child = None;
            }
        }
        unrecord_pid(self.pid);
        // The supervisor thread may have reaped it first; either way it's gone.
        let until = Instant::now() + Duration::from_secs(10);
        while self.exit_reason().is_none() {
            if Instant::now() > until {
                return Err(RuntimeError::Io("process did not exit".into()));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        let _ = self.terminate(Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_sleep_command() -> (Command, &'static str) {
        #[cfg(windows)]
        {
            let mut c = Command::new("cmd");
            c.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
            (c, "cmd")
        }
        #[cfg(not(windows))]
        {
            let mut c = Command::new("sleep");
            c.arg("30");
            (c, "sleep")
        }
    }

    #[test]
    fn registry_roundtrip_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(REGISTRY_FILE);
        record_pid_in(&file, 4242, "/opt/bin/llama-server.exe");
        let list = read_records(&file);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].pid, 4242);
        assert_eq!(list[0].exe_name, "llama-server");

        unrecord_pid_in(&file, 4242);
        assert!(!file.exists(), "empty registry file is removed cleanly");

        std::fs::write(&file, b"{corrupt json").unwrap();
        assert!(read_records(&file).is_empty());
        assert!(!file.exists());
    }

    #[test]
    fn cleanup_reaps_allowed_worker_and_ignores_unrelated_pid_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(REGISTRY_FILE);

        // Spawn a real child without attaching Supervised so it acts like an orphan.
        let (mut cmd, stem) = dummy_sleep_command();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let pid = child.id();

        // When the allowlist does NOT permit this executable (PID reuse guard),
        // cleanup MUST refuse to terminate the process.
        record_pid_in(&file, pid, stem);
        let killed_wrong = cleanup_stale_workers_for(dir.path(), &["llama-server"]);
        assert_eq!(killed_wrong, 0);
        assert!(
            child.try_wait().unwrap().is_none(),
            "unrelated process left untouched"
        );

        // When the recorded exe matches the allowlist AND the live OS process stem,
        // cleanup terminates it cleanly.
        record_pid_in(&file, pid, stem);
        let killed_match = cleanup_stale_workers_for(dir.path(), &[stem]);
        assert_eq!(killed_match, 1);
        assert!(child.wait().is_ok());
        assert!(!file.exists());
    }

    #[test]
    fn supervised_records_and_unrecords_own_worker() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(REGISTRY_FILE);
        let (mut cmd, stem) = dummy_sleep_command();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let sup = Supervised::spawn(cmd).unwrap();
        let pid = sup.pid();
        record_pid_in(&file, pid, stem);
        assert_eq!(read_records(&file).len(), 1);
        sup.terminate(Duration::from_millis(200)).unwrap();
        unrecord_pid_in(&file, pid);
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn terminate_reaps_and_reports_exit() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo hello; sleep 30"]);
        let s = Supervised::spawn(cmd).unwrap();
        assert!(s.exit_reason().is_none());
        let until = Instant::now() + Duration::from_secs(5);
        while s.log_tail(5).is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.log_tail(5), vec!["hello".to_string()]);
        let t = Instant::now();
        s.terminate(Duration::from_millis(500)).unwrap();
        assert!(s.exit_reason().is_some());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn natural_exit_is_detected() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 3"]);
        let s = Supervised::spawn(cmd).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while s.exit_reason().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(s.exit_reason().unwrap().contains('3'));
    }

    #[test]
    fn spawn_failure_is_load_failed() {
        let r = Supervised::spawn(Command::new("/nonexistent/binary"));
        assert!(matches!(r, Err(RuntimeError::LoadFailed(_))));
    }

    #[cfg(unix)]
    #[test]
    fn drop_kills_the_child() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let s = Supervised::spawn(cmd).unwrap();
        let pid = s.pid();
        drop(s);
        #[cfg(target_os = "linux")]
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}/cmdline")).exists()
                || std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| s.contains(") Z"))
                    .unwrap_or(true)
        );
        #[cfg(target_os = "macos")]
        {
            // SAFETY: signal 0 checks process existence without delivering a signal.
            let alive = unsafe { libc_kill(pid as i32, 0) } == 0;
            assert!(!alive, "child pid {pid} still alive after drop");
        }
    }
}
