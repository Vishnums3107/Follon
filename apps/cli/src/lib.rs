//! Shared operator-CLI primitives for immutable local research artifacts.

use std::fs;
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Publishes one immutable file atomically and treats an identical repeat as idempotent.
///
/// A same-directory staging file is fully synced before a hard link makes the
/// final name visible. The link operation cannot overwrite an existing file,
/// which keeps concurrent publishers fail-closed.
pub fn write_immutable(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    // A link at the artifact path is refused, dangling or not. `exists()`
    // follows a link, so a dangling one looked absent: the staging file was
    // written, the publish then failed, and the staging file was left behind.
    // A link to identical content counted as already published (delivery
    // state E7.1, E3.11's rule).
    refuse_symbolic_link(path)?;
    if path.exists() {
        return if follon_file_safety::read_to_string(path)? == contents {
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
    let mut temporary_file = follon_file_safety::create_new(&temporary).map_err(|error| {
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
            let existing = follon_file_safety::read_to_string(path);
            fs::remove_file(&temporary)?;
            if existing? == contents {
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

/// Refuses a symbolic link at an output path, dangling or not, without
/// following it. An absent path passes; any other error is returned.
pub fn refuse_symbolic_link(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing to write through a symbolic link: {}",
            path.display()
        )
        .into()),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
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

    #[test]
    fn immutable_writer_refuses_a_symbolic_link_and_stages_nothing() {
        let directory =
            std::env::temp_dir().join(format!("follon-immutable-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("elsewhere.json");
        let link = directory.join("artifact.json");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link);
        if let Err(error) = linked {
            eprintln!("cannot create a symbolic link ({error}); the refusal was not exercised");
            std::fs::remove_dir_all(&directory).unwrap();
            return;
        }
        let only_the_link = |directory: &Path| {
            let mut names = std::fs::read_dir(directory)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect::<Vec<_>>();
            names.sort();
            names
        };

        // Dangling: refused as a link, not by whatever failed later, and no
        // staging file is left beside it.
        let error = write_immutable(&link, "contents").unwrap_err().to_string();
        assert!(error.contains("symbolic link"), "{error}");
        assert!(
            !target.exists(),
            "the artifact was written through the link"
        );
        assert_eq!(only_the_link(&directory), vec!["artifact.json".to_owned()]);

        // A link to identical content used to count as already published.
        std::fs::write(&target, "contents").unwrap();
        let error = write_immutable(&link, "contents").unwrap_err().to_string();
        assert!(error.contains("symbolic link"), "{error}");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
