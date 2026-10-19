//! Runtime dispatch.
//!
//! [`Runtime`] is the single place that knows which sandbox a manifest asked
//! for. The rest of the plugin subsystem talks to it through three hooks, so a
//! Lua plugin and a WebAssembly plugin are indistinguishable to the manager.

use std::path::Path;
use std::sync::Arc;

use super::PluginError;
use super::manifest::{Manifest, RuntimeKind};
use super::HostBridge;

/// The three entry points a plugin may implement. All are optional except
/// `on_event`, which is optional too — a load-only plugin is legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    /// Called once after the module is instantiated, with the initial config.
    Load,
    /// Called for every playback event, with `{ kind, payload }`.
    Event,
    /// Called after the user changes a setting.
    Config,
}

impl Hook {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Load => "on_load",
            Self::Event => "on_event",
            Self::Config => "on_config",
        }
    }
}

/// A loaded plugin instance.
#[derive(Debug)]
pub enum Runtime {
    #[cfg(feature = "lua-plugins")]
    Lua(super::lua_rt::LuaRuntime),
    #[cfg(feature = "wasm-plugins")]
    Wasm(super::wasm_rt::WasmRuntime),
}

impl Runtime {
    /// Build the sandbox the manifest asks for.
    ///
    /// A build without the relevant feature returns an error that names the
    /// feature instead of pretending the plugin loaded.
    pub fn spawn(
        manifest: &Manifest,
        dir: &Path,
        bridge: Arc<HostBridge>,
    ) -> Result<Self, PluginError> {
        match manifest.runtime {
            RuntimeKind::Lua => {
                #[cfg(feature = "lua-plugins")]
                {
                    let runtime = super::lua_rt::LuaRuntime::new(manifest, dir, bridge)?;
                    Ok(Self::Lua(runtime))
                }
                #[cfg(not(feature = "lua-plugins"))]
                {
                    let _ = (dir, bridge);
                    Err(PluginError::runtime(format!(
                        "plugin '{}' needs the Lua runtime, which this build does not include \
                         (rebuild with --features lua-plugins)",
                        manifest.id
                    )))
                }
            }
            RuntimeKind::Wasm => {
                #[cfg(feature = "wasm-plugins")]
                {
                    let runtime = super::wasm_rt::WasmRuntime::new(manifest, dir, bridge)?;
                    Ok(Self::Wasm(runtime))
                }
                #[cfg(not(feature = "wasm-plugins"))]
                {
                    let _ = (dir, bridge);
                    Err(PluginError::runtime(format!(
                        "plugin '{}' needs the WebAssembly runtime, which this build does not include \
                         (rebuild with --features wasm-plugins)",
                        manifest.id
                    )))
                }
            }
        }
    }

    pub fn on_load(&mut self, config: &str) -> Result<(), PluginError> {
        match self {
            #[cfg(feature = "lua-plugins")]
            Self::Lua(runtime) => runtime.call(Hook::Load, config),
            #[cfg(feature = "wasm-plugins")]
            Self::Wasm(runtime) => runtime.call(Hook::Load, config),
            #[allow(unreachable_patterns)]
            _ => {
                let _ = (config, self);
                Ok(())
            }
        }
    }

    pub fn on_event(&mut self, payload: &str) -> Result<(), PluginError> {
        match self {
            #[cfg(feature = "lua-plugins")]
            Self::Lua(runtime) => runtime.call(Hook::Event, payload),
            #[cfg(feature = "wasm-plugins")]
            Self::Wasm(runtime) => runtime.call(Hook::Event, payload),
            #[allow(unreachable_patterns)]
            _ => {
                let _ = (payload, self);
                Ok(())
            },
        }
    }

    pub fn on_config(&mut self, payload: &str) -> Result<(), PluginError> {
        match self {
            #[cfg(feature = "lua-plugins")]
            Self::Lua(runtime) => runtime.call(Hook::Config, payload),
            #[cfg(feature = "wasm-plugins")]
            Self::Wasm(runtime) => runtime.call(Hook::Config, payload),
            #[allow(unreachable_patterns)]
            _ => {
                let _ = (payload, self);
                Ok(())
            }
        }
    }
}
