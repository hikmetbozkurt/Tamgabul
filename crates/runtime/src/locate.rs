//! Finding the llama.cpp `llama-server` executable.
//!
//! Order: explicit path (Settings) → `TAMGABUL_LLAMA_SERVER` → next to the app
//! (bundled, ADR 019) → `PATH` → winget install locations (Windows).
//! Nothing is downloaded; if nothing is found the caller reports it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::RuntimeError;
use crate::process::hide_console;

pub const ENV_OVERRIDE: &str = "TAMGABUL_LLAMA_SERVER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Explicit,
    Env,
    Bundled,
    Path,
    Winget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub source: Source,
}

pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn usable(p: &Path) -> bool {
    p.is_file()
}

pub fn find_llama_server(explicit: Option<&Path>) -> Result<Found, RuntimeError> {
    if let Some(p) = explicit {
        // An explicit choice is never silently replaced by another binary.
        return if usable(p) {
            Ok(Found {
                path: p.to_path_buf(),
                source: Source::Explicit,
            })
        } else {
            Err(RuntimeError::RuntimeNotFound)
        };
    }
    if let Some(p) = std::env::var_os(ENV_OVERRIDE).map(PathBuf::from) {
        if usable(&p) {
            return Ok(Found {
                path: p,
                source: Source::Env,
            });
        }
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        for p in [dir.join(exe_name()), dir.join("llama").join(exe_name())] {
            if usable(&p) {
                return Ok(Found {
                    path: p,
                    source: Source::Bundled,
                });
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe_name());
            if usable(&p) {
                return Ok(Found {
                    path: p,
                    source: Source::Path,
                });
            }
        }
    }
    if let Some(p) = winget_candidates().into_iter().find(|p| usable(p)) {
        return Ok(Found {
            path: p,
            source: Source::Winget,
        });
    }
    Err(RuntimeError::RuntimeNotFound)
}

/// winget puts a link in `%LOCALAPPDATA%\Microsoft\WinGet\Links` and the real
/// files in `...\WinGet\Packages\ggml.llamacpp_<source>\`.
fn winget_candidates() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    let base = local.join("Microsoft").join("WinGet");
    let mut v = vec![base.join("Links").join(exe_name())];
    if let Ok(rd) = std::fs::read_dir(base.join("Packages")) {
        let mut pkgs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().starts_with("ggml.llamacpp"))
            })
            .collect();
        pkgs.sort();
        v.extend(pkgs.into_iter().map(|p| p.join(exe_name())));
    }
    v
}

/// `llama-server --version`, first line like `version: 6912 (a1b2c3d)`.
pub fn version(exe: &Path) -> Result<String, RuntimeError> {
    let mut cmd = Command::new(exe);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| RuntimeError::LoadFailed(format!("cannot run runtime: {e}")))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::Timeout);
            }
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| RuntimeError::Io(e.to_string()))?;
    // llama.cpp prints the version to stderr.
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    parse_version(&text).ok_or_else(|| RuntimeError::BadResponse("no version line".into()))
}

pub fn parse_version(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("version:"))
        .map(|l| l.trim_start_matches("version:").trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_is_not_replaced() {
        assert_eq!(
            find_llama_server(Some(Path::new("/nope/llama-server"))),
            Err(RuntimeError::RuntimeNotFound)
        );
    }

    #[test]
    fn explicit_existing_wins() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(exe_name());
        std::fs::write(&p, b"").unwrap();
        let f = find_llama_server(Some(&p)).unwrap();
        assert_eq!(f.source, Source::Explicit);
    }

    #[test]
    fn parses_version_line() {
        let s = "ggml_cuda_init: found 1 CUDA devices\nversion: 6912 (a1b2c3d)\nbuilt with MSVC 19.4 for x64\n";
        assert_eq!(parse_version(s).as_deref(), Some("6912 (a1b2c3d)"));
        assert_eq!(parse_version("nothing"), None);
    }
}
