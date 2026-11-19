//! Finding external runtime executables (`llama-server`, `whisper-server`, `piper`).
//!
//! Order: explicit path (Settings) → env override → next to the app (bundled)
//! → `PATH` → Homebrew (macOS) → winget (Windows).
//! Nothing is downloaded; if nothing is found the caller reports it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::RuntimeError;
use crate::process::hide_console;

pub const ENV_OVERRIDE: &str = "TAMGABUL_LLAMA_SERVER";
pub const WHISPER_ENV_OVERRIDE: &str = "TAMGABUL_WHISPER_SERVER";
pub const PIPER_ENV_OVERRIDE: &str = "TAMGABUL_PIPER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Explicit,
    Env,
    Bundled,
    Path,
    Homebrew,
    Winget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub source: Source,
}

fn bin_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

pub fn whisper_exe_name() -> &'static str {
    if cfg!(windows) {
        "whisper-server.exe"
    } else {
        "whisper-server"
    }
}

pub fn piper_exe_name() -> &'static str {
    if cfg!(windows) {
        "piper.exe"
    } else {
        "piper"
    }
}

fn usable(p: &Path) -> bool {
    p.is_file()
}

/// Paths checked relative to the running app binary for a bundled runtime:
/// adjacent directory, `<subdir>/`, and macOS `.app/Contents/Resources/{,<subdir>/}`.
pub fn bundled_candidates_for(exe_dir: &Path, file_name: &str, subdir: &str) -> Vec<PathBuf> {
    let mut out = vec![
        exe_dir.join(file_name),
        exe_dir.join(subdir).join(file_name),
    ];
    if let Some(contents) = exe_dir.parent() {
        let resources = contents.join("Resources");
        out.push(resources.join(file_name));
        out.push(resources.join(subdir).join(file_name));
    }
    out
}

pub fn bundled_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    bundled_candidates_for(exe_dir, exe_name(), "llama")
}

#[cfg(target_os = "macos")]
fn homebrew_candidates(file_name: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin").join(file_name),
        PathBuf::from("/usr/local/bin").join(file_name),
    ]
}

#[cfg(not(target_os = "macos"))]
fn homebrew_candidates(_file_name: &str) -> Vec<PathBuf> {
    Vec::new()
}

/// winget puts a link in `%LOCALAPPDATA%\Microsoft\WinGet\Links` and package
/// folders under `...\WinGet\Packages\<prefix>*\`.
#[cfg(windows)]
fn winget_candidates(file_name: &str, prefixes: &[&str]) -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    let base = local.join("Microsoft").join("WinGet");
    let mut v = vec![base.join("Links").join(file_name)];
    if !prefixes.is_empty() {
        if let Ok(rd) = std::fs::read_dir(base.join("Packages")) {
            let mut pkgs: Vec<PathBuf> = rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        let low = n.to_ascii_lowercase();
                        prefixes.iter().any(|pre| low.starts_with(pre))
                    })
                })
                .collect();
            pkgs.sort();
            v.extend(pkgs.into_iter().map(|p| p.join(file_name)));
        }
    }
    v
}

#[cfg(not(windows))]
fn winget_candidates(_file_name: &str, _prefixes: &[&str]) -> Vec<PathBuf> {
    Vec::new()
}

fn find_binary(
    explicit: Option<&Path>,
    env_override: &str,
    base_name: &str,
    subdir: &str,
    winget_prefixes: &[&str],
) -> Result<Found, RuntimeError> {
    let name = bin_name(base_name);
    if let Some(p) = explicit {
        return if usable(p) {
            Ok(Found {
                path: p.to_path_buf(),
                source: Source::Explicit,
            })
        } else {
            Err(RuntimeError::RuntimeNotFound)
        };
    }
    if let Some(p) = std::env::var_os(env_override).map(PathBuf::from) {
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
        if let Some(p) = bundled_candidates_for(&dir, &name, subdir)
            .into_iter()
            .find(|p| usable(p))
        {
            return Ok(Found {
                path: p,
                source: Source::Bundled,
            });
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(&name);
            if usable(&p) {
                return Ok(Found {
                    path: p,
                    source: Source::Path,
                });
            }
        }
    }
    if let Some(p) = homebrew_candidates(&name).into_iter().find(|p| usable(p)) {
        return Ok(Found {
            path: p,
            source: Source::Homebrew,
        });
    }
    if let Some(p) = winget_candidates(&name, winget_prefixes)
        .into_iter()
        .find(|p| usable(p))
    {
        return Ok(Found {
            path: p,
            source: Source::Winget,
        });
    }
    Err(RuntimeError::RuntimeNotFound)
}

pub fn find_llama_server(explicit: Option<&Path>) -> Result<Found, RuntimeError> {
    find_binary(
        explicit,
        ENV_OVERRIDE,
        "llama-server",
        "llama",
        &["ggml.llamacpp"],
    )
}

pub fn find_whisper_server(explicit: Option<&Path>) -> Result<Found, RuntimeError> {
    find_binary(
        explicit,
        WHISPER_ENV_OVERRIDE,
        "whisper-server",
        "whisper",
        &["ggml.whisper"],
    )
}

pub fn find_piper(explicit: Option<&Path>) -> Result<Found, RuntimeError> {
    find_binary(explicit, PIPER_ENV_OVERRIDE, "piper", "piper", &[])
}

fn run_flag_probe(exe: &Path, args: &[&str]) -> Result<(bool, String), RuntimeError> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    Ok((out.status.success(), text))
}

/// `llama-server --version`, first line like `version: 6912 (a1b2c3d)`.
pub fn version(exe: &Path) -> Result<String, RuntimeError> {
    let (_, text) = run_flag_probe(exe, &["--version"])?;
    parse_version(&text).ok_or_else(|| RuntimeError::BadResponse("no version line".into()))
}

/// Probe `whisper-server --help` (whisper.cpp server prints usage and exits 0 on `--help`).
pub fn probe_whisper_server(exe: &Path) -> Result<String, RuntimeError> {
    let (ok, text) = run_flag_probe(exe, &["--help"])?;
    if !ok && !text.contains("--inference-path") {
        return Err(RuntimeError::BadResponse(
            "invalid whisper-server binary".into(),
        ));
    }
    Ok(parse_version(&text).unwrap_or_else(|| "whisper.cpp".into()))
}

/// Probe `piper --version` (or fallback to `--help` on `piper1-gpl`).
pub fn probe_piper(exe: &Path) -> Result<String, RuntimeError> {
    if let Ok((true, text)) = run_flag_probe(exe, &["--version"]) {
        if let Some(first) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
            return Ok(first.chars().take(40).collect());
        }
    }
    let (ok, text) = run_flag_probe(exe, &["--help"])?;
    if !ok && !text.contains("--model") {
        return Err(RuntimeError::BadResponse("invalid piper binary".into()));
    }
    Ok("piper".into())
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
        assert_eq!(
            find_whisper_server(Some(Path::new("/nope/whisper-server"))),
            Err(RuntimeError::RuntimeNotFound)
        );
        assert_eq!(
            find_piper(Some(Path::new("/nope/piper"))),
            Err(RuntimeError::RuntimeNotFound)
        );
    }

    #[test]
    fn explicit_existing_wins_across_all_runtimes() {
        let d = tempfile::tempdir().unwrap();
        let l = d.path().join(exe_name());
        let w = d.path().join(whisper_exe_name());
        let p = d.path().join(piper_exe_name());
        std::fs::write(&l, b"").unwrap();
        std::fs::write(&w, b"").unwrap();
        std::fs::write(&p, b"").unwrap();
        assert_eq!(
            find_llama_server(Some(&l)).unwrap().source,
            Source::Explicit
        );
        assert_eq!(
            find_whisper_server(Some(&w)).unwrap().source,
            Source::Explicit
        );
        assert_eq!(find_piper(Some(&p)).unwrap().source, Source::Explicit);
    }

    #[test]
    fn parses_version_line() {
        let s = "ggml_cuda_init: found 1 CUDA devices\nversion: 6912 (a1b2c3d)\nbuilt with MSVC 19.4 for x64\n";
        assert_eq!(parse_version(s).as_deref(), Some("6912 (a1b2c3d)"));
        assert_eq!(parse_version("nothing"), None);
    }

    #[test]
    fn bundled_candidates_cover_windows_and_macos_app_bundle() {
        let exe_dir = Path::new("/Applications/Tamgabul.app/Contents/MacOS");
        let cands = bundled_candidates(exe_dir);
        assert!(cands.contains(&exe_dir.join(exe_name())));
        assert!(cands.contains(&exe_dir.join("llama").join(exe_name())));
        assert!(cands.contains(
            &PathBuf::from("/Applications/Tamgabul.app/Contents/Resources/llama").join(exe_name())
        ));
        let whisper_cands = bundled_candidates_for(exe_dir, whisper_exe_name(), "whisper");
        assert!(whisper_cands.contains(
            &PathBuf::from("/Applications/Tamgabul.app/Contents/Resources/whisper")
                .join(whisper_exe_name())
        ));
    }
}
