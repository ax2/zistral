//! User-selected result files. This grants no filesystem tools to the model.
use anyhow::{Context, Result, bail};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, Permissions},
    hash::Hasher,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAX_FILE: u64 = 64 * 1024 * 1024;

#[derive(PartialEq, Eq)]
struct Revision {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    hash: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn observe(path: &Path, allow_existing: bool) -> Result<Option<(Revision, Permissions)>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("cannot inspect output target"),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("output target must be a regular file, not a directory, link or device");
    }
    if !allow_existing {
        bail!("output file already exists; use --force to replace it");
    }
    if metadata.permissions().readonly() {
        bail!("output target is read-only");
    }
    if metadata.len() > MAX_FILE {
        bail!("existing output file exceeds 64 MiB");
    }
    let mut file = File::open(path).context("cannot read existing output for change detection")?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        bail!("output target is no longer a regular file");
    }
    let mut hash = DefaultHasher::new();
    let mut length = 0u64;
    let mut bytes = [0; 16384];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        length += n as u64;
        if length > MAX_FILE {
            bail!("existing output file exceeds 64 MiB");
        }
        hash.write(&bytes[..n]);
    }
    let after = file.metadata()?;
    if length != opened.len()
        || after.len() != opened.len()
        || after.modified().ok() != opened.modified().ok()
    {
        bail!("output changed while inspecting it; retry with a stable file");
    }
    Ok(Some((
        Revision {
            length,
            modified: after.modified().ok(),
            created: after.created().ok(),
            hash: hash.finish(),
            #[cfg(unix)]
            mode: {
                use std::os::unix::fs::PermissionsExt;
                after.permissions().mode()
            },
            #[cfg(unix)]
            device: {
                use std::os::unix::fs::MetadataExt;
                after.dev()
            },
            #[cfg(unix)]
            inode: {
                use std::os::unix::fs::MetadataExt;
                after.ino()
            },
        },
        after.permissions(),
    )))
}

fn validate_name(path: &Path) -> Result<()> {
    let raw = path.as_os_str().to_string_lossy();
    if matches!(
        raw.rsplit(std::path::is_separator).next(),
        Some("" | "." | "..") | None
    ) {
        bail!("output must end with a file name, not a directory separator or dot component");
    }
    let name = path.file_name().context("output must name a file")?;
    #[cfg(windows)]
    {
        let name = name.to_string_lossy();
        let stem = name.split('.').next().unwrap_or("").to_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p).is_some_and(|s| {
                matches!(
                    s,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if device
            || name.ends_with(['.', ' '])
            || name
                .chars()
                .any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
        {
            bail!(
                "output requires an ordinary Windows file name, not a device or alternate data stream"
            );
        }
    }
    #[cfg(not(windows))]
    let _ = name;
    Ok(())
}

pub struct Prepared {
    target: PathBuf,
    original: Option<(Revision, Permissions)>,
    temporary: tempfile::NamedTempFile,
}

impl Prepared {
    pub fn new(path: &Path, force: bool) -> Result<Self> {
        validate_name(path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = parent
            .canonicalize()
            .context("output parent directory must already exist")?;
        if !parent.is_dir() {
            bail!("output parent must be a directory");
        }
        let target = parent.join(path.file_name().context("output must name a file")?);
        let original = observe(&target, force)?;
        let temporary = tempfile::Builder::new()
            .prefix(".zi-result-")
            .suffix(".tmp")
            .tempfile_in(&parent)
            .context("cannot create temporary result file in output directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            target,
            original,
            temporary,
        })
    }

    /// The caller selects stdout's existing newline convention; an empty data transform stays empty.
    pub fn save(mut self, text: &str, append_newline: bool) -> Result<usize> {
        let length = text
            .len()
            .checked_add(usize::from(append_newline))
            .context("result size overflow")?;
        if length as u64 > MAX_FILE {
            bail!("result file exceeds 64 MiB; nothing published");
        }
        self.temporary
            .write_all(text.as_bytes())
            .context("cannot write temporary result")?;
        if append_newline {
            self.temporary.write_all(b"\n")?;
        }
        self.temporary
            .as_file()
            .sync_all()
            .context("cannot flush temporary result")?;
        let current = observe(&self.target, true)?;
        if current.as_ref().map(|(r, _)| r) != self.original.as_ref().map(|(r, _)| r) {
            bail!("output target changed during the task; nothing replaced");
        }
        #[cfg(unix)]
        if let Some((_, permissions)) = &self.original {
            use std::os::unix::fs::PermissionsExt;
            self.temporary
                .as_file()
                .set_permissions(Permissions::from_mode(permissions.mode() & 0o777))?;
            self.temporary.as_file().sync_all()?;
        }
        if self.original.is_some() {
            self.temporary
                .persist(&self.target)
                .map_err(|e| e.error)
                .context("cannot replace output file")?;
        } else {
            self.temporary
                .persist_noclobber(&self.target)
                .map_err(|e| e.error)
                .context(
                    "cannot publish output (existing files are never replaced without --force)",
                )?;
        }
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_publication_no_clobber_cleanup_and_observed_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let pending = Prepared::new(&path, false).unwrap();
        assert!(!path.exists());
        drop(pending);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        Prepared::new(&path, false)
            .unwrap()
            .save("你好", true)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), "你好\n".as_bytes());
        assert!(Prepared::new(&path, false).is_err());
        let pending = Prepared::new(&path, true).unwrap();
        fs::write(&path, "external").unwrap();
        assert!(pending.save("new", true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external");
        Prepared::new(&path, true).unwrap().save("", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"");
        fs::remove_file(&path).unwrap();
        let pending = Prepared::new(&path, true).unwrap();
        fs::write(&path, "raced").unwrap();
        assert!(pending.save("new", true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"raced");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn directories_readonly_and_missing_parents_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Prepared::new(dir.path(), true).is_err());
        assert!(Prepared::new(&dir.path().join("missing/out"), false).is_err());
        for tail in ["new-directory/", "new-directory/.", "new-directory/.."] {
            assert!(Prepared::new(&dir.path().join(tail), false).is_err());
        }
        assert!(!dir.path().join("new-directory").exists());
        let path = dir.path().join("read-only");
        fs::write(&path, "keep").unwrap();
        let original = fs::metadata(&path).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        assert!(Prepared::new(&path, true).is_err());
        fs::set_permissions(&path, original).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(Prepared::new(&path, true).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn unix_links_are_rejected_and_basic_permissions_are_preserved() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out");
        Prepared::new(&path, false)
            .unwrap()
            .save("new", true)
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, Permissions::from_mode(0o750)).unwrap();
        Prepared::new(&path, true)
            .unwrap()
            .save("changed", true)
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o750
        );
        let alias = dir.path().join("alias");
        fs::hard_link(&path, &alias).unwrap();
        let pending = Prepared::new(&path, true).unwrap();
        assert_eq!(
            pending
                .temporary
                .as_file()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        pending.save("last", true).unwrap();
        assert_eq!(fs::read(&alias).unwrap(), b"changed\n");
        assert_eq!(fs::read(&path).unwrap(), b"last\n");
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(Prepared::new(&link, true).is_err());
        fs::remove_file(&path).unwrap();
        assert!(Prepared::new(&link, true).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_device_names_and_alternate_streams_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "NUL", "nul.txt", "COM1", "LPT³", "CONOUT$", "a:stream", "a.", "a ",
        ] {
            assert!(
                Prepared::new(&dir.path().join(name), true).is_err(),
                "{name}"
            );
        }
    }
}
