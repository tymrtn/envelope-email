// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! Descriptor-relative output helpers for hostile export destinations.
//!
//! Paths are resolved one component at a time below an already-open directory.
//! This prevents a check-then-use symlink swap from redirecting an evidence
//! export. Files are published with `linkat` rather than a replacing rename, so
//! an existing output (including a symlink) is never overwritten.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct SecureOutputDir {
    fd: OwnedFd,
}

#[cfg(unix)]
impl SecureOutputDir {
    /// Open `path`, creating missing components without ever following one.
    pub(crate) fn open_or_create(path: &Path) -> io::Result<Self> {
        let mut dir = match path.components().next() {
            Some(Component::RootDir) => Self::open_initial(b"/")?,
            _ => Self::open_initial(b".")?,
        };
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => dir = dir.open_or_create_child_os(name)?,
                Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "output path must not contain parent components",
                    ));
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported output path prefix",
                    ));
                }
            }
        }
        Ok(dir)
    }

    fn open_initial(name: &[u8]) -> io::Result<Self> {
        let name = cstring(name)?;
        // `.` is resolved by the kernel from the already-selected working
        // directory. All caller-controlled components below it use openat with
        // O_NOFOLLOW.
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        owned_fd(fd)
    }

    pub(crate) fn open_or_create_child(&self, name: &str) -> io::Result<Self> {
        self.open_or_create_child_os(std::ffi::OsStr::new(name))
    }

    fn open_or_create_child_os(&self, name: &std::ffi::OsStr) -> io::Result<Self> {
        let name = cstring(name.as_bytes())?;
        let result = unsafe { libc::mkdirat(self.fd.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        owned_fd(fd)
    }

    /// Create a random exclusive temp file, fsync it, then atomically publish it
    /// without replacing any existing final name. Both operations use this
    /// directory's descriptor rather than a pathname that can be swapped.
    pub(crate) fn write_new_atomic(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        validate_file_name(name)?;
        let final_name = cstring(name.as_bytes())?;
        for _ in 0..32 {
            let temp_name = format!(".{name}.{}.tmp", uuid::Uuid::new_v4());
            let temp_name_c = cstring(temp_name.as_bytes())?;
            let fd = unsafe {
                libc::openat(
                    self.fd.as_raw_fd(),
                    temp_name_c.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }

            let write_result = (|| {
                let mut file = unsafe { File::from_raw_fd(fd) };
                file.write_all(bytes)?;
                file.sync_all()?;
                // Drop closes the descriptor before publication; the directory
                // capability remains held by `self` for the link operation.
                drop(file);
                let link_result = unsafe {
                    libc::linkat(
                        self.fd.as_raw_fd(),
                        temp_name_c.as_ptr(),
                        self.fd.as_raw_fd(),
                        final_name.as_ptr(),
                        0,
                    )
                };
                if link_result != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            })();
            // A temp file is never a final artifact. Ignore cleanup failure only
            // when the preceding operation already failed, preserving its cause.
            let cleanup_result =
                unsafe { libc::unlinkat(self.fd.as_raw_fd(), temp_name_c.as_ptr(), 0) };
            match write_result {
                Ok(()) => {
                    if cleanup_result != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate an exclusive evidence export temp file",
        ))
    }

    /// Read an existing regular file without following a symlink. `None` means
    /// the name is absent; non-regular files are rejected rather than read.
    pub(crate) fn read_regular(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        validate_file_name(name)?;
        let name = cstring(name.as_bytes())?;
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "evidence export target exists but is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        let mut file = file;
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }
}

#[cfg(unix)]
fn cstring(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

#[cfg(unix)]
fn owned_fd(fd: libc::c_int) -> io::Result<SecureOutputDir> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(SecureOutputDir {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
}

#[cfg(unix)]
fn validate_file_name(name: &str) -> io::Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains('\0')
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "evidence export file name must be one normal path component",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
#[derive(Debug)]
pub(crate) struct SecureOutputDir;

#[cfg(not(unix))]
impl SecureOutputDir {
    pub(crate) fn open_or_create(_: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "race-safe evidence export requires Unix descriptor-relative filesystem APIs",
        ))
    }

    pub(crate) fn open_or_create_child(&self, _: &str) -> io::Result<Self> {
        Self::open_or_create(Path::new(""))
    }

    pub(crate) fn write_new_atomic(&self, _: &str, _: &[u8]) -> io::Result<()> {
        Self::open_or_create(Path::new(""))?;
        unreachable!()
    }

    pub(crate) fn read_regular(&self, _: &str) -> io::Result<Option<Vec<u8>>> {
        Self::open_or_create(Path::new(""))?;
        unreachable!()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_symlinked_parent_without_touching_external_target() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = base.path().join("swap");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();

        assert!(SecureOutputDir::open_or_create(&link.join("bundle")).is_err());
        assert!(!outside.path().join("bundle").exists());
    }

    #[test]
    fn does_not_replace_a_symlink_final_target() {
        let out = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), b"outside evidence").unwrap();
        std::os::unix::fs::symlink(outside.path(), out.path().join("manifest.json")).unwrap();

        let dir = SecureOutputDir::open_or_create(out.path()).unwrap();
        let error = dir
            .write_new_atomic("manifest.json", b"new evidence")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(outside.path()).unwrap(), b"outside evidence");
    }

    #[test]
    fn ignores_stale_predictable_temp_name_and_publishes_from_random_exclusive_temp() {
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("manifest.json.tmp"), b"stale temp").unwrap();
        let dir = SecureOutputDir::open_or_create(out.path()).unwrap();

        dir.write_new_atomic("manifest.json", b"canonical evidence")
            .unwrap();

        assert_eq!(
            fs::read(out.path().join("manifest.json")).unwrap(),
            b"canonical evidence"
        );
        assert_eq!(
            fs::read(out.path().join("manifest.json.tmp")).unwrap(),
            b"stale temp"
        );
    }
}
