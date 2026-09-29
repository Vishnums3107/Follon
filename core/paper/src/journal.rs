//! The append-only, hash-chained, single-process PAPER journal file.

use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::*;

const PAPER_JOURNAL_SCHEMA_VERSION: u32 = 3;
const LEGACY_PAPER_JOURNAL_SCHEMA_VERSION: u32 = 2;
const MAX_PAPER_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

/// Durable append-only state journal for one paper OMS account.
///
/// Every line is a canonical complete snapshot. Retaining full snapshots makes
/// restart recovery fail-closed and auditable without relying on a mutable
/// database row. A deployment may rotate this local adapter into versioned
/// object storage after preserving its immutable sequence.
pub struct FilePaperJournal {
    path: PathBuf,
    file: File,
    next_sequence: u64,
    previous_hash: String,
    latest: Option<PersistentPaperState>,
}

impl FilePaperJournal {
    /// Opens and verifies an existing journal before accepting new snapshots.
    /// An absent journal is created empty, with its directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PaperError> {
        Self::open_with(path.as_ref(), true)?
            .ok_or_else(|| PaperError("paper journal was not created".to_owned()))
    }

    /// Opens and verifies a journal, creating it when absent only if `create`
    /// is set. Otherwise an absent journal is `None` and nothing is created,
    /// neither the file nor its directory.
    pub(crate) fn open_with(path: &Path, create: bool) -> Result<Option<Self>, PaperError> {
        let path = path.to_path_buf();
        // `symlink_metadata` never follows a link, so a dangling one is
        // refused too. The `exists()` check this replaced followed it, found
        // nothing, and the open below created the journal at its target
        // (E3.11).
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PaperError(
                    "paper journal path must not be a symbolic link".to_owned(),
                ));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(PaperError(error.to_string()));
            }
            _ => {}
        }
        if create {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| PaperError(error.to_string()))?;
            }
        }
        let mut file = match OpenOptions::new()
            .create(create)
            .read(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(PaperError(error.to_string())),
        };
        file.try_lock_exclusive().map_err(|error| {
            PaperError(format!(
                "paper journal is already open by another operator/process: {error}"
            ))
        })?;
        let mut latest = None;
        let mut next_sequence = 1;
        let mut previous_hash = "0".repeat(64);
        let mut modern_record_seen = false;
        let byte_count = file
            .metadata()
            .map_err(|error| PaperError(error.to_string()))?
            .len();
        if byte_count > MAX_PAPER_JOURNAL_BYTES {
            return Err(PaperError(format!(
                "paper journal exceeds the {} byte recovery limit; rotate and archive it before restart",
                MAX_PAPER_JOURNAL_BYTES
            )));
        }
        if byte_count > 0 {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|error| PaperError(error.to_string()))?;
            for (index, line) in contents.lines().enumerate() {
                if line.is_empty() {
                    return Err(PaperError(format!(
                        "paper journal contains an empty line at {}",
                        index + 1
                    )));
                }
                let record: PersistentJournalRecord =
                    serde_json::from_str(line).map_err(|error| {
                        PaperError(format!("invalid paper journal line {}: {error}", index + 1))
                    })?;
                if serde_json::to_string(&record).map_err(|error| PaperError(error.to_string()))?
                    != line
                {
                    return Err(PaperError(format!(
                        "paper journal line {} is not canonical JSON",
                        index + 1
                    )));
                }
                match record {
                    PersistentJournalRecord::V3(record) => {
                        if record.schema_version != PAPER_JOURNAL_SCHEMA_VERSION
                            || record.sequence != next_sequence
                            || record.previous_hash != previous_hash
                            || record.entry_hash != paper_record_hash(&record)?
                        {
                            return Err(PaperError(format!(
                                "paper journal integrity check failed at line {}",
                                index + 1
                            )));
                        }
                        modern_record_seen = true;
                        previous_hash = record.entry_hash;
                        latest = Some(record.state);
                    }
                    PersistentJournalRecord::V2(record) => {
                        if modern_record_seen
                            || record.schema_version != LEGACY_PAPER_JOURNAL_SCHEMA_VERSION
                            || record.sequence != next_sequence
                        {
                            return Err(PaperError(format!(
                                "paper journal legacy record is invalid at line {}",
                                index + 1
                            )));
                        }
                        previous_hash = legacy_paper_record_hash(&previous_hash, line);
                        latest = Some(record.state);
                    }
                }
                next_sequence += 1;
            }
        }
        Ok(Some(Self {
            path,
            file,
            next_sequence,
            previous_hash,
            latest,
        }))
    }

    /// Returns the journal location for deployment backup and restore controls.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn latest(&self) -> Option<&PersistentPaperState> {
        self.latest.as_ref()
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    pub(crate) fn head_hash(&self) -> &str {
        &self.previous_hash
    }

    pub(crate) fn append(&mut self, state: PersistentPaperState) -> Result<(), PaperError> {
        let mut record = PersistentJournalRecordV3 {
            schema_version: PAPER_JOURNAL_SCHEMA_VERSION,
            sequence: self.next_sequence,
            previous_hash: self.previous_hash.clone(),
            state,
            entry_hash: String::new(),
        };
        record.entry_hash = paper_record_hash(&record)?;
        let serialized = serde_json::to_string(&PersistentJournalRecord::V3(record.clone()))
            .map_err(|error| PaperError(error.to_string()))?;
        self.file
            .write_all(serialized.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.sync_data())
            .map_err(|error| PaperError(error.to_string()))?;
        self.next_sequence += 1;
        self.previous_hash = record.entry_hash;
        self.latest = Some(record.state);
        Ok(())
    }
}

fn paper_record_hash(record: &PersistentJournalRecordV3) -> Result<String, PaperError> {
    let mut unsigned = record.clone();
    unsigned.entry_hash.clear();
    let canonical = serde_json::to_string(&PersistentJournalRecord::V3(unsigned))
        .map_err(|error| PaperError(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn legacy_paper_record_hash(previous_hash: &str, canonical_line: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"follon-paper-journal-v2-anchor");
    hasher.update(previous_hash.as_bytes());
    hasher.update((canonical_line.len() as u64).to_be_bytes());
    hasher.update(canonical_line.as_bytes());
    format!("{:x}", hasher.finalize())
}
