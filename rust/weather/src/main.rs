//! Tempo na Waybar (custom/weather): o porte de `home/.local/bin/waybar-weather-bar`, que passa a vez a
//! este binário (sway_rust). A Waybar o roda a cada minuto; uma linha e sai.
use std::process::ExitCode;

use swaydesk_weather::{bar_line, fetch, Now, Settings};

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-weather\n",
    "    Prints the weather line for Waybar (custom/weather) and exits.",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-weather-bar em Python confere que o binário carrega
    }
    println!("{}", bar_line(&Settings::load(), &Now::local(), fetch));
    ExitCode::SUCCESS
}
