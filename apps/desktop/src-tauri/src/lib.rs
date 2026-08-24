//! Minimal native host for the React operations client.

/// Runs the Tauri application with no privileged commands exposed to web code.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("Follon desktop runtime failed");
}
