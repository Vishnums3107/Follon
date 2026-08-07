//! Shared operator-CLI primitives for immutable local research artifacts.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Publishes one immutable file atomically and treats an identical repeat as idempotent.
///
/// A same-directory staging file is fully synced before a hard link makes the
/// final name visible. The link operation cannot overwrite an existing file,
/// which keeps concurrent publishers fail-closed.
pub fn write_immutable(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return if fs::read_to_string(path)? == contents {
            Ok(())
        } else {
            Err(format!(
                "refusing to overwrite immutable artifact: {}",
                path.display()
            )
            .into())
        };
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("immutable artifact path must have a UTF-8 file name")?;
    let digest = sha256_text(contents);
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        &digest[..16]
    ));
    let mut temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            format!(
                "cannot create immutable artifact staging file {}: {error}",
                temporary.display()
            )
        })?;
    if let Err(error) = temporary_file
        .write_all(contents.as_bytes())
        .and_then(|_| temporary_file.sync_data())
    {
        drop(temporary_file);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(temporary_file);
    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            fs::remove_file(&temporary)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing_matches = fs::read_to_string(path)? == contents;
            fs::remove_file(&temporary)?;
            if existing_matches {
                Ok(())
            } else {
                Err(format!(
                    "refusing to overwrite immutable artifact: {}",
                    path.display()
                )
                .into())
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(format!(
                "cannot atomically publish immutable artifact {}: {error}",
                path.display()
            )
            .into())
        }
    }
}

/// Returns a lowercase SHA-256 digest for exact UTF-8 artifact bytes.
pub fn sha256_text(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_writer_is_idempotent_and_rejects_conflicts() {
        let path = std::env::temp_dir().join(format!(
            "follon-immutable-artifact-{}-{}.json",
            std::process::id(),
            "shared-writer"
        ));
        let _ = std::fs::remove_file(&path);
        write_immutable(&path, "first").unwrap();
        write_immutable(&path, "first").unwrap();
        assert!(write_immutable(&path, "different").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
