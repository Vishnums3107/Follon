# Regular evidence files

`follon-file-safety` owns final-component file-open safety for local evidence.
Unix opens use `O_NOFOLLOW | O_NONBLOCK`; Windows opens use
`FILE_FLAG_OPEN_REPARSE_POINT` and anonymous security quality of service.
The handle's own metadata must identify a regular file, and Windows refuses
every reparse point. Truncation occurs only after this validation. Exclusive
creation uses `create_new`, and an absent read remains `NotFound`.

The OS behavior is documented by
[Rust's Unix extensions](https://doc.rust-lang.org/std/os/unix/fs/trait.OpenOptionsExt.html),
[Rust's Windows extensions](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html),
and [Microsoft's CreateFile contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).
In particular, `OPEN_REPARSE_POINT` cannot accompany `CREATE_ALWAYS`; the
truncating helper opens without truncation, validates, then calls `set_len(0)`.

Existing path checks may remain to provide useful error messages. The safe
open is authoritative even if a link is planted after that check. Callers keep
ownership of file locking, journal validation, size bounds, syncing and recovery.
No persisted format, configuration fingerprint or replay arithmetic changes.

Parent directories must be trusted and protected. This crate does not stop
parent-directory replacement, hard links, mutation by an authorized writer,
or hostile device names from blocking a Windows open. It is not a security
sandbox or a general-purpose filesystem confinement library.

Run `cargo test -p follon-file-safety`. Link tests require Windows Developer
Mode or symlink privilege and fail if the prerequisite is missing. They never
report an unexercised guard as passing. The foundation Rust job runs the same
tests on Ubuntu; local Windows evidence alone does not verify Unix behavior.
