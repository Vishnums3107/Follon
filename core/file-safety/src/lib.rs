//! Handle-validated regular-file access for local evidence stores.
//!
//! Opens never follow the final path component. Parent directories must remain
//! trusted and access controlled; this is not directory traversal confinement,
//! a defence against hard links, or an exclusive-writer lock.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

/// Access to a regular evidence file, without implicit truncation.
#[derive(Clone, Copy, Debug)]
pub enum Access {
    /// Read an existing file.
    Read,
    /// Write from the beginning without truncating.
    Write,
    /// Read and write at explicit offsets.
    ReadWrite,
    /// Append only.
    Append,
    /// Read existing contents and append new evidence.
    ReadAppend,
}

/// Opens a regular file without following its final path component.
///
/// The type check uses metadata from the opened handle, never a second path
/// lookup. Creation is optional and never truncates an existing object.
pub fn open(path: impl AsRef<Path>, access: Access, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(create);
    match access {
        Access::Read => {
            options.read(true);
        }
        Access::Write => {
            options.write(true);
        }
        Access::ReadWrite => {
            options.read(true).write(true);
        }
        Access::Append => {
            options.append(true);
        }
        Access::ReadAppend => {
            options.read(true).append(true);
        }
    }
    open_checked(path.as_ref(), options)
}

/// Creates a new regular file exclusively, refusing any existing name.
pub fn create_new(path: impl AsRef<Path>) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    open_checked(path.as_ref(), options)
}

/// Opens and validates the handle before truncating the regular file.
pub fn create_truncated(path: impl AsRef<Path>) -> io::Result<File> {
    let file = open(path, Access::Write, true)?;
    file.set_len(0)?;
    Ok(file)
}

/// Reads UTF-8 contents from a regular file without following a final link.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let mut contents = String::new();
    open(path, Access::Read, false)?.read_to_string(&mut contents)?;
    Ok(contents)
}

fn open_checked(path: &Path, mut options: OpenOptions) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // NONBLOCK prevents a planted FIFO from hanging before its type can
        // be checked. It does not change regular-file I/O semantics.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        // Explicit anonymous SQOS prevents named-pipe impersonation before
        // the handle type is known. Rust adds SECURITY_SQOS_PRESENT.
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(0);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no-follow evidence files require a supported platform",
    ));

    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "evidence file must not be a symbolic link or reparse point",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "evidence handle must refer to a regular file",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "follon-file-safety-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn regular_files_preserve_append_exclusive_create_and_truncation() {
        let scratch = Scratch::new();
        let path = scratch.0.join("evidence");
        create_new(&path).unwrap().write_all(b"first").unwrap();
        assert!(create_new(&path).is_err());
        open(&path, Access::ReadWrite, true).unwrap();
        open(&path, Access::Append, true)
            .unwrap()
            .write_all(b"second")
            .unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "firstsecond");
        create_truncated(&path).unwrap().write_all(b"new").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn absent_files_and_directories_are_not_evidence() {
        let scratch = Scratch::new();
        assert_eq!(
            open(scratch.0.join("missing"), Access::Read, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(open(&scratch.0, Access::Read, false).is_err());
    }

    #[cfg(any(unix, windows))]
    fn link(target: &Path, path: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, path).unwrap();
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn links_cannot_be_read_appended_truncated_or_used_to_create_targets() {
        let scratch = Scratch::new();
        let target = scratch.0.join("target");
        std::fs::write(&target, "unchanged").unwrap();
        let linked = scratch.0.join("linked");
        link(&target, &linked);
        for access in [
            Access::Read,
            Access::Write,
            Access::Append,
            Access::ReadWrite,
            Access::ReadAppend,
        ] {
            assert!(open(&linked, access, true).is_err(), "{access:?}");
        }
        assert!(read_to_string(&linked).is_err());
        assert!(create_new(&linked).is_err());
        assert!(create_truncated(&linked).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "unchanged");

        let missing = scratch.0.join("missing");
        let dangling = scratch.0.join("dangling");
        link(&missing, &dangling);
        assert!(open(&dangling, Access::ReadAppend, true).is_err());
        assert!(create_truncated(&dangling).is_err());
        assert!(!missing.exists());
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn a_link_planted_after_a_successful_path_check_is_refused() {
        let scratch = Scratch::new();
        let path = scratch.0.join("journal");
        let target = scratch.0.join("other-account");
        std::fs::write(&path, "initial").unwrap();
        std::fs::write(&target, "protected").unwrap();
        assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
        std::fs::remove_file(&path).unwrap();
        link(&target, &path);
        assert!(open(&path, Access::ReadAppend, true).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "protected");
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn an_open_handle_keeps_its_identity_when_its_path_is_replaced() {
        let scratch = Scratch::new();
        let path = scratch.0.join("journal");
        let retained = scratch.0.join("retained");
        let target = scratch.0.join("protected");
        std::fs::write(&path, "original").unwrap();
        std::fs::write(&target, "protected").unwrap();
        let mut file = open(&path, Access::ReadAppend, false).unwrap();
        std::fs::rename(&path, &retained).unwrap();
        link(&target, &path);
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "original");
        file.write_all(b"-appended").unwrap();
        file.sync_data().unwrap();
        assert_eq!(
            std::fs::read_to_string(&retained).unwrap(),
            "original-appended"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "protected");
        assert!(open(&path, Access::ReadAppend, true).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        use std::sync::mpsc;
        use std::time::Duration;

        let scratch = Scratch::new();
        let path = scratch.0.join("fifo");
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender
                .send(open(&path, Access::Read, false).is_err())
                .unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }
}
