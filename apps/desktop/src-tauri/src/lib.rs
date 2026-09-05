//! Native host for the React trading terminal.

pub mod trading;

/// Runs the Tauri application and exposes its bounded trading commands.
pub fn run() {
    tauri::Builder::default()
        .manage(trading::TradingCommandState::unavailable())
        .invoke_handler(tauri::generate_handler![
            trading::submit_order,
            trading::cancel_order,
            trading::close_position,
            trading::trading_command_status
        ])
        .run(tauri::generate_context!())
        .expect("Follon desktop runtime failed");
}
