// Keeps a console window from opening next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--mcp-stdio`: relay an MCP client to the running app, with no window.
    if let Some(code) = windfall_desktop_lib::mcp::stdio_entry() {
        std::process::exit(code);
    }
    if windfall_desktop_lib::plugins::helper_entry() {
        return;
    }
    if windfall_desktop_lib::plugins::scanner_entry() {
        return;
    }
    windfall_desktop_lib::run();
}
