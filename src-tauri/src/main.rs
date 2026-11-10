// Release builds are window apps. Without this, Windows would open a console alongside them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    disweep_app::run();
}
