#![cfg_attr(not(test), windows_subsystem = "windows")]

mod core;
mod ui;

use crate::core::elevation::{APPLY_MACHINE_ARG, run_helper};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = if args.first().map(String::as_str) == Some(APPLY_MACHINE_ARG) {
        run_helper(&args)
    } else {
        ui::run(args.first().cloned())
    };
    std::process::exit(code);
}
