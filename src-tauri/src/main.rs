// Suppress the extra console window that Windows allocates for GUI processes
// in release builds. Debug builds keep the console so `tracing` output and
// panics are visible during development and CI smoke tests.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    prism_lib::run();
}
