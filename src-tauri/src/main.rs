// Suppress the extra console window that Windows allocates for GUI processes
// in release builds. Debug builds keep the console so `tracing` output and
// panics are visible during development and CI smoke tests.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(unsafe_code)] // Environment is set before Prism starts any threads.
fn main() {
    // WebKitGTK on NVIDIA/Wayland can abort on explicit-sync protocol errors.
    // Respect an explicit user setting and apply the workaround only when the
    // NVIDIA driver and a Wayland session are present, before threads start.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::path::Path::new("/proc/driver/nvidia/version").exists()
        && std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none()
    {
        // SAFETY: this is the first operation in main, before Prism starts any
        // threads or initializes GTK/WebKit.
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
    }
    // The AppImage bundles WebKitGTK and can encounter incompatible DMA-BUF
    // drivers. Keep native packages unchanged and respect a user override.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        // SAFETY: GTK/WebKit and application threads have not started yet.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    prism_lib::run();
}
