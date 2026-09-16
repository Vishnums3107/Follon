//! Native host for the React trading terminal.

pub mod paper_gateway;
pub mod trading;

use std::sync::Arc;

/// Runs the Tauri application and exposes its bounded trading commands.
pub fn run() {
    let trading_state = match paper_gateway::bootstrap() {
        Some(gateway) => trading::TradingCommandState::with_gateway(Arc::new(gateway)),
        None => trading::TradingCommandState::unavailable(),
    };
    tauri::Builder::default()
        .manage(trading_state)
        .invoke_handler(tauri::generate_handler![
            trading::submit_order,
            trading::cancel_order,
            trading::close_position,
            trading::trading_command_status
        ])
        .run(tauri::generate_context!())
        .expect("Follon desktop runtime failed");
}
