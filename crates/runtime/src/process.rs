//! Child process supervision shared by every worker.
//!
//! Guarantees (docs/05):
//! - Windows: the child is placed in a Job Object with KILL_ON_JOB_CLOSE, so
//!   it dies with the app even if the app crashes or is killed. No console
//!   window is created.
//! - Linux: PR_SET_PDEATHSIG(SIGKILL). The child is spawned from a dedicated
//!   supervisor thread that lives exactly as long as the child (the signal is
//!   tied to the spawning *thread*).
//! - macOS: no kernel-level tie exists; the app's explicit shutdown path
//!   terminates workers. Recorded as a known gap in docs/19.
//! - stdout/stderr are drained continuously (a full pipe would block the
//!   worker) into a small ring buffer used only for error reports.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::RuntimeError;

const LOG_LINES: usize = 80;

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

#[derive(Default)]
struct Shared {
    child: Option<Child>,
    exit: Option<String>,
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
                    shared.lock().expect("worker state").child = Some(child);
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
                                return;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                s.exit = Some(format!("wait failed: {e}"));
                                s.child = None;
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
        if let Some(c) = s.child.as_mut() {
            if let Ok(Some(status)) = c.try_wait() {
                s.exit = Some(status.to_string());
                s.child = None;
            }
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
        #[cfg(unix)]
        if !grace.is_zero() && self.exit_reason().is_none() {
            // SAFETY: plain kill(2) on a pid we spawned and still own.
            unsafe {
                libc_kill(self.pid as i32, 15);
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

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn terminate_reaps_and_reports_exit() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo hello; sleep 30"]);
        let s = Supervised::spawn(cmd).unwrap();
        assert!(s.exit_reason().is_none());
        // stdout is drained into the log while the process runs.
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

    #[test]
    fn drop_kills_the_child() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let s = Supervised::spawn(cmd).unwrap();
        let pid = s.pid();
        drop(s);
        // /proc entry disappears (or becomes a non-running state) once reaped.
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}/cmdline")).exists()
                || std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| s.contains(") Z"))
                    .unwrap_or(true)
        );
    }
}
