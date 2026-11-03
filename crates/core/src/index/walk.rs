//! Path safety and bounded, read-only folder traversal (docs/07).
//!
//! - Roots are canonicalized; network (UNC) roots are refused for now.
//! - Never follows symlinks, junctions or other reparse-point directories,
//!   so a scan cannot escape its root or loop.
//! - Skips cloud placeholders (OneDrive "online-only" files) so a scan never
//!   triggers downloads.
//! - Default exclusions: OS folders, VCS/dependency/build/cache folders,
//!   hidden folders, and caller-supplied deny paths (app data, model folders,
//!   persistent user exclusions).
//! - Caps on files, depth and time; a capped preview reports `complete=false`.
//! - Only opens directories for listing; file contents are not read here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime};

use crate::error::{ErrorCode, IpcError};

/// Folder names skipped anywhere (case-insensitive).
pub const EXCLUDED_DIR_NAMES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "target",
    ".cache",
    ".idea",
    ".vs",
    ".vscode",
    ".next",
    ".gradle",
    "$recycle.bin",
    "system volume information",
    "appdata",
];

/// Extra folders skipped directly under a drive root.
pub const DRIVE_ROOT_EXCLUDES: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "recovery",
    "perflogs",
    "$windows.~bt",
    "$windows.~ws",
    "$winreagent",
    "boot",
    "efi",
    // unix-like roots
    "proc",
    "sys",
    "dev",
    "usr",
    "bin",
    "sbin",
    "lib",
    "etc",
    "var",
    "private",
    "system",
    "library",
];

/// File kinds indexed today. Images go to the vision encoder; PDFs are
/// text-extracted page by page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Markdown,
    Text,
    Pdf,
    Image,
}

impl SourceKind {
    pub fn from_path(p: &Path) -> Option<Self> {
        let ext = p.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "md" | "markdown" => Some(SourceKind::Markdown),
            "txt" => Some(SourceKind::Text),
            "pdf" => Some(SourceKind::Pdf),
            // Formats the llama.cpp image loader (stb_image) decodes.
            "png" | "jpg" | "jpeg" | "bmp" | "gif" => Some(SourceKind::Image),
            _ => None,
        }
    }
    pub fn as_db_str(self) -> &'static str {
        match self {
            SourceKind::Markdown => "markdown",
            SourceKind::Text => "text",
            SourceKind::Pdf => "pdf",
            SourceKind::Image => "image",
        }
    }
    /// Size cap per kind; `text_cap` is the (configurable) text limit.
    pub fn max_bytes(self, text_cap: u64) -> u64 {
        match self {
            SourceKind::Markdown | SourceKind::Text => text_cap,
            SourceKind::Pdf => 64 * 1024 * 1024,
            SourceKind::Image => 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_files: usize,
    pub max_depth: usize,
    /// Cap for text files (PDF/image caps are fixed per kind).
    pub max_file_bytes: u64,
    pub deadline: Option<Instant>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 200_000,
            max_depth: 40,
            max_file_bytes: 10 * 1024 * 1024,
            deadline: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: PathBuf,
    pub kind: SourceKind,
    pub size: u64,
    pub mtime: SystemTime,
}

#[derive(Debug, Default, Clone)]
pub struct WalkReport {
    pub files: Vec<Candidate>,
    pub total_bytes: u64,
    /// False when a cap, the deadline or cancel stopped the walk early.
    pub complete: bool,
    /// Distinct excluded folder names that were actually encountered.
    pub excluded_names: Vec<String>,
    pub skipped_large: u32,
    pub skipped_links: u32,
    pub skipped_cloud: u32,
    pub unreadable_dirs: u32,
}

/// Canonical form used for storage and ancestry checks. On Windows the
/// `\\?\` prefix is removed for normal drive paths (std re-adds it when a
/// long path needs it).
pub fn canonical(p: &Path) -> Result<PathBuf, IpcError> {
    let c = std::fs::canonicalize(p).map_err(|_| {
        IpcError::new(
            ErrorCode::ScopeNotAuthorized,
            "path not found or not readable",
        )
    })?;
    Ok(simplify(c))
}

fn simplify(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC\\") {
                return PathBuf::from(rest.to_string());
            }
        }
    }
    p
}

pub fn is_network_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with(r"\\") && !s.starts_with(r"\\?\") || s.starts_with(r"\\?\UNC\")
}

pub fn is_drive_root(p: &Path) -> bool {
    p.parent().is_none()
}

/// Is `path` equal to or inside `root`? Component-wise, case-insensitive on
/// Windows. Both must be canonical.
pub fn is_within(path: &Path, root: &Path) -> bool {
    if cfg!(windows) {
        let a = path.to_string_lossy().to_lowercase();
        let b = root.to_string_lossy().to_lowercase();
        Path::new(&a).starts_with(Path::new(&b))
    } else {
        path.starts_with(root)
    }
}

pub fn display(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn attrs(md: &std::fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes()
}
#[cfg(not(windows))]
fn attrs(_md: &std::fs::Metadata) -> u32 {
    0
}

const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;
const ATTR_REPARSE: u32 = 0x400;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x4_0000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

fn excluded_name(name: &str, at_drive_root: bool) -> bool {
    let n = name.to_lowercase();
    EXCLUDED_DIR_NAMES.contains(&n.as_str())
        || (at_drive_root && DRIVE_ROOT_EXCLUDES.contains(&n.as_str()))
}

/// Walk `root` (canonical). `deny` are canonical paths never entered.
pub fn walk(root: &Path, deny: &[PathBuf], limits: Limits, cancel: &AtomicBool) -> WalkReport {
    let mut r = WalkReport {
        complete: true,
        ..Default::default()
    };
    let root_is_drive = is_drive_root(root);
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let note_excluded = |name: &str, r: &mut WalkReport| {
        if !r
            .excluded_names
            .iter()
            .any(|x| x.eq_ignore_ascii_case(name))
        {
            r.excluded_names.push(name.to_string());
        }
    };

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || limits.deadline.is_some_and(|d| Instant::now() > d) {
            r.complete = false;
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            r.unreadable_dirs += 1;
            continue;
        };
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            // symlink_metadata: never follows links.
            let Ok(md) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = md.file_type();
            let a = attrs(&md);
            if ft.is_symlink() {
                r.skipped_links += 1;
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                if a & ATTR_REPARSE != 0 {
                    r.skipped_links += 1;
                    continue;
                }
                let at_root = root_is_drive && depth == 0;
                if excluded_name(&name, at_root)
                    || name.starts_with('.')
                    || (a & ATTR_HIDDEN != 0 && a & ATTR_SYSTEM != 0)
                {
                    note_excluded(&name, &mut r);
                    continue;
                }
                if deny.iter().any(|d| is_within(&path, d)) {
                    continue;
                }
                if depth < limits.max_depth {
                    subdirs.push(path);
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let Some(kind) = SourceKind::from_path(&path) else {
                continue;
            };
            if a & (ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS) != 0 {
                r.skipped_cloud += 1;
                continue;
            }
            if md.len() > kind.max_bytes(limits.max_file_bytes) {
                r.skipped_large += 1;
                continue;
            }
            if r.files.len() >= limits.max_files {
                r.complete = false;
                return finish(r);
            }
            r.total_bytes += md.len();
            r.files.push(Candidate {
                path,
                kind,
                size: md.len(),
                mtime: md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        // Deterministic order: process subfolders alphabetically.
        subdirs.sort();
        for d in subdirs.into_iter().rev() {
            stack.push((d, depth + 1));
        }
    }
    finish(r)
}

fn finish(mut r: WalkReport) -> WalkReport {
    r.excluded_names.sort_by_key(|s| s.to_lowercase());
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::create_dir_all(p.join("notes/sub")).unwrap();
        fs::create_dir_all(p.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(p.join(".git")).unwrap();
        fs::create_dir_all(p.join("Build")).unwrap();
        fs::write(p.join("a.md"), "# A").unwrap();
        fs::write(p.join("notes/b.txt"), "b").unwrap();
        fs::write(p.join("notes/sub/c.MD"), "c").unwrap();
        fs::write(p.join("notes/image.png"), "x").unwrap();
        fs::write(p.join("notes/report.PDF"), "%PDF").unwrap();
        fs::write(p.join("notes/data.docx"), "x").unwrap();
        fs::write(p.join("node_modules/pkg/readme.md"), "no").unwrap();
        fs::write(p.join(".git/HEAD.txt"), "no").unwrap();
        fs::write(p.join("Build/out.txt"), "no").unwrap();
        d
    }

    fn names(r: &WalkReport) -> Vec<String> {
        let mut v: Vec<String> = r
            .files
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn finds_supported_files_and_skips_default_exclusions() {
        let d = tree();
        let root = canonical(d.path()).unwrap();
        let r = walk(&root, &[], Limits::default(), &AtomicBool::new(false));
        assert!(r.complete);
        assert_eq!(
            names(&r),
            ["a.md", "b.txt", "c.MD", "image.png", "report.PDF"]
        );
        assert_eq!(r.excluded_names, [".git", "Build", "node_modules"]);
        assert_eq!(r.total_bytes, 10);
    }

    #[test]
    fn caps_mark_incomplete() {
        let d = tree();
        let root = canonical(d.path()).unwrap();
        let r = walk(
            &root,
            &[],
            Limits {
                max_files: 2,
                ..Limits::default()
            },
            &AtomicBool::new(false),
        );
        assert!(!r.complete);
        assert_eq!(r.files.len(), 2);
        let r = walk(&root, &[], Limits::default(), &AtomicBool::new(true));
        assert!(!r.complete);
    }

    #[test]
    fn deny_paths_and_size_limit() {
        let d = tree();
        let root = canonical(d.path()).unwrap();
        let deny = vec![canonical(&d.path().join("notes/sub")).unwrap()];
        let r = walk(
            &root,
            &deny,
            Limits {
                max_file_bytes: 1,
                ..Limits::default()
            },
            &AtomicBool::new(false),
        );
        // a.md is 3 bytes → too large for the text cap; c.MD is denied;
        // image/PDF caps are separate.
        assert_eq!(names(&r), ["b.txt", "image.png", "report.PDF"]);
        assert_eq!(r.skipped_large, 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_never_escape_the_root_or_loop() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "s").unwrap();
        let d = tree();
        std::os::unix::fs::symlink(outside.path(), d.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(d.path(), d.path().join("notes/loop")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.md"), d.path().join("link.md"))
            .unwrap();
        let root = canonical(d.path()).unwrap();
        let r = walk(&root, &[], Limits::default(), &AtomicBool::new(false));
        assert_eq!(
            names(&r),
            ["a.md", "b.txt", "c.MD", "image.png", "report.PDF"]
        );
        assert_eq!(r.skipped_links, 3);
    }

    #[test]
    fn ancestry() {
        assert!(is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a/bc"), Path::new("/a/b")));
        assert!(is_drive_root(Path::new("/")));
        assert!(!is_drive_root(Path::new("/home")));
        assert!(is_network_path(Path::new(r"\\server\share")));
        assert!(!is_network_path(Path::new(r"C:\x")));
    }

    #[test]
    fn missing_root_is_not_authorized() {
        assert_eq!(
            canonical(Path::new("/definitely/missing"))
                .unwrap_err()
                .code,
            ErrorCode::ScopeNotAuthorized
        );
    }
}
