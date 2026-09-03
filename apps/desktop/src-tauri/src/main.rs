//! Native executable entrypoint.
//!
//! The library run function registers the bounded submit, cancel, and
//! position-close commands before starting the Tauri runtime.

fn main() {
    follon_desktop_lib::run();
}
