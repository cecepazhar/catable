#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V" || a == "-v") {
        println!("catable {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    catable_app_lib::run();
}
