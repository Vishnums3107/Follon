//! The checked-in PAPER journal fixture must load under the current format.
//!
//! `FilePaperJournal::open` requires every line to re-serialize byte for byte,
//! so adding a field to the persisted state silently invalidates this fixture.
//! That has happened twice (conformance-audit items 34 and 51), each time
//! noticed only when the evidence pipeline next ran. This test makes
//! `cargo test` notice instead.

use std::fs;
use std::path::Path;

use follon_paper::FilePaperJournal;

#[test]
fn checked_in_paper_journal_fixture_loads_under_the_current_format() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/paper/journal-v2.ndjson");
    // Opened through a copy: a test must never be able to modify the fixture.
    let scratch =
        std::env::temp_dir().join(format!("follon-paper-fixture-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    let copy = scratch.join("journal-v2.ndjson");
    fs::copy(&fixture, &copy).unwrap();

    let result = FilePaperJournal::open(&copy);
    let _ = fs::remove_dir_all(&scratch);
    if let Err(error) = result {
        panic!(
            "tests/fixtures/paper/journal-v2.ndjson no longer loads ({error}). Regenerate it with \
             the current follon-paper-status binary against tests/fixtures/config/paper-v2.json, \
             never by hand -- see conformance-audit item 34."
        );
    }
}
