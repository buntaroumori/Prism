//! Build script for the Prism desktop shell.
//!
//! Responsibilities:
//!   * `tauri_build::build()` embeds `tauri.conf.json`, validates it and
//!     generates the capability/ACL schemas consumed by the frontend.
//!   * SQL migrations are embedded at compile time by `sqlx::migrate!`, so the
//!     binary can never boot with a missing or partially copied migration set.
//!     Rebuild whenever a migration changes.

fn main() {
    tauri_build::build();

    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=capabilities");
}
