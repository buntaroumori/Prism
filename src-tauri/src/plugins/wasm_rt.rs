//! WebAssembly sandbox (`wasm-plugins` feature).
//!
//! Modules are compiled by Cranelift and instantiated with **no WASI**: the only
//! imports a plugin can resolve are the five host functions below, exported
//! from the `prism` module. There is no filesystem, no clock, no random source
//! and no network unless the host hands one over.
//!
//! ## ABI
//!
//! Memory: a module must export `memory`, `alloc(len: i32) -> i32` and an
//! `on_event(ptr, len)` function. `on_load(ptr, len)` and `on_config(ptr, len)`
//! are optional. Payloads are UTF-8 JSON written by the host into memory
//! returned by `alloc`.
//!
//! Every call is metered: the store is refuelled with [`WASM_FUEL_PER_CALL`]
//! before each hook, so a module cannot spin forever.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail};
use wasmtime::{
    Caller, Config, Engine, Extern, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

use super::manifest::Manifest;
use super::{ControlOp, Hook, HostBridge, PluginError, WASM_FUEL_PER_CALL, WASM_MEMORY_LIMIT};

/// Largest accepted module file (a plugin is not a game engine).
const MAX_MODULE_BYTES: usize = 8 * 1024 * 1024;
/// Instructions a module may execute while being compiled/linked at startup.
const STARTUP_FUEL: u64 = 5_000_000_000;

/// Per-instance store data: resource limits plus the host bridge.
struct StoreData {
    limits: StoreLimits,
    bridge: Arc<HostBridge>,
}

impl std::fmt::Debug for StoreData {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoreData")
            .field("bridge", &self.bridge)
            .finish_non_exhaustive()
    }
}

impl StoreData {
    fn new(bridge: Arc<HostBridge>) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(WASM_MEMORY_LIMIT)
                .instances(1)
                .tables(4)
                .build(),
            bridge,
        }
    }
}

pub(crate) struct WasmRuntime {
    store: Store<StoreData>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    on_load: Option<TypedFunc<(i32, i32), ()>>,
    on_event: Option<TypedFunc<(i32, i32), ()>>,
    on_config: Option<TypedFunc<(i32, i32), ()>>,
    #[allow(dead_code)]
    instance: Instance,
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasmRuntime")
            .field("on_load", &self.on_load.is_some())
            .field("on_event", &self.on_event.is_some())
            .field("on_config", &self.on_config.is_some())
            .finish_non_exhaustive()
    }
}

impl WasmRuntime {
    pub(crate) fn new(
        manifest: &Manifest,
        dir: &Path,
        bridge: Arc<HostBridge>,
    ) -> Result<Self, PluginError> {
        let path = dir.join(&manifest.entry);
        let bytes = std::fs::read(&path).map_err(|error| PluginError::io(path.clone(), error))?;
        if bytes.len() > MAX_MODULE_BYTES {
            return Err(PluginError::manifest(format!(
                "{} exceeds {} MiB",
                path.display(),
                MAX_MODULE_BYTES / (1024 * 1024)
            )));
        }

        let mut config = Config::new();
        config.consume_fuel(true);
        // Fuel is the watchdog; epochs would need a ticking thread per instance.
        config.epoch_interruption(false);
        let engine = Engine::new(&config)
            .map_err(|error| PluginError::runtime(format!("cannot create wasm engine: {error}")))?;

        let module = Module::new(&engine, &bytes).map_err(|error| {
            PluginError::runtime(format!("{} is not a valid module: {error}", path.display()))
        })?;

        let mut linker: Linker<StoreData> = Linker::new(&engine);
        register_imports(&mut linker)?;

        let mut store = Store::new(&engine, StoreData::new(bridge));
        store.limiter(|data| &mut data.limits);
        store
            .set_fuel(STARTUP_FUEL)
            .map_err(|error| PluginError::runtime(format!("cannot fuel store: {error}")))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|error| PluginError::runtime(format!("cannot instantiate plugin: {error}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| PluginError::manifest("module does not export 'memory'"))?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|_| PluginError::manifest("module does not export alloc(len: i32) -> i32"))?;

        let on_load = optional(instance.get_typed_func::<(i32, i32), ()>(&mut store, "on_load"))?;
        let on_event = optional(instance.get_typed_func::<(i32, i32), ()>(&mut store, "on_event"))?;
        let on_config =
            optional(instance.get_typed_func::<(i32, i32), ()>(&mut store, "on_config"))?;

        // Drop the startup allowance down to a per-call budget.
        store
            .set_fuel(WASM_FUEL_PER_CALL)
            .map_err(|error| PluginError::runtime(format!("cannot fuel store: {error}")))?;

        Ok(Self {
            store,
            memory,
            alloc,
            on_load,
            on_event,
            on_config,
            instance,
        })
    }

    pub(crate) fn call(&mut self, hook: Hook, payload: &str) -> Result<(), PluginError> {
        let function = match hook {
            Hook::Load => self.on_load.as_ref(),
            Hook::Event => self.on_event.as_ref(),
            Hook::Config => self.on_config.as_ref(),
        };
        let Some(function) = function else {
            return Ok(());
        };

        self.store.set_fuel(WASM_FUEL_PER_CALL).map_err(|error| {
            PluginError::runtime(format!(
                "cannot refuel plugin for {}: {error}",
                hook.as_str()
            ))
        })?;

        let bytes = payload.as_bytes();
        let length = i32::try_from(bytes.len())
            .map_err(|_| PluginError::runtime("payload too large for the plugin ABI"))?;
        let pointer = self
            .alloc
            .call(&mut self.store, length)
            .map_err(|error| wrap(hook, error))?;
        if pointer <= 0 {
            return Err(PluginError::runtime(format!(
                "{}: alloc({length}) returned an invalid pointer",
                hook.as_str()
            )));
        }
        let start = usize::try_from(pointer)
            .map_err(|_| PluginError::runtime("alloc returned a negative pointer"))?;
        let data = self.memory.data_mut(&mut self.store);
        let end = start
            .checked_add(bytes.len())
            .filter(|end| *end <= data.len())
            .ok_or_else(|| {
                PluginError::runtime(format!(
                    "{}: alloc returned memory outside the module's data",
                    hook.as_str()
                ))
            })?;
        data[start..end].copy_from_slice(bytes);

        function
            .call(&mut self.store, (pointer, length))
            .map_err(|error| wrap(hook, error))
    }
}

fn wrap(hook: Hook, error: wasmtime::Error) -> PluginError {
    PluginError::runtime(format!("{} handler failed: {error}", hook.as_str()))
}

fn optional<T>(result: Result<T, wasmtime::Error>) -> Result<Option<T>, PluginError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

/// Register the host imports. Anything a module asks for that is not listed
/// here fails instantiation — the sandbox is the import list.
fn register_imports(linker: &mut Linker<StoreData>) -> Result<(), PluginError> {
    let map =
        |error: wasmtime::Error| PluginError::runtime(format!("cannot link host api: {error}"));

    linker
        .func_wrap(
            "prism",
            "log",
            |mut caller: Caller<'_, StoreData>,
             level: i32,
             ptr: i32,
             len: i32|
             -> wasmtime::Result<()> {
                let text = read_string(&mut caller, ptr, len)?;
                let level = match level {
                    0 => "debug",
                    2 => "warn",
                    3 => "error",
                    _ => "info",
                };
                let bridge = caller.data().bridge.clone();
                if let Err(error) = bridge.log(level, &text) {
                    bail!("{error}");
                }
                Ok(())
            },
        )
        .map_err(map)?;

    linker
        .func_wrap(
            "prism",
            "emit",
            |mut caller: Caller<'_, StoreData>, ptr: i32, len: i32| -> wasmtime::Result<()> {
                let text = read_string(&mut caller, ptr, len)?;
                let bridge = caller.data().bridge.clone();
                if let Err(error) = bridge.notify(None, &text) {
                    bail!("{error}");
                }
                Ok(())
            },
        )
        .map_err(map)?;

    linker
        .func_wrap(
            "prism",
            "now_playing",
            |mut caller: Caller<'_, StoreData>, ptr: i32, cap: i32| -> wasmtime::Result<i32> {
                let bridge = caller.data().bridge.clone();
                let text = match bridge.now_playing() {
                    Ok(text) => text,
                    Err(error) => bail!("{error}"),
                };
                write_string(&mut caller, ptr, cap, &text)
            },
        )
        .map_err(map)?;

    linker
        .func_wrap(
            "prism",
            "config",
            |mut caller: Caller<'_, StoreData>, ptr: i32, cap: i32| -> wasmtime::Result<i32> {
                let bridge = caller.data().bridge.clone();
                let text = bridge.config_json();
                write_string(&mut caller, ptr, cap, &text)
            },
        )
        .map_err(map)?;

    linker
        .func_wrap(
            "prism",
            "control",
            |caller: Caller<'_, StoreData>, op: i32, value: f64| -> wasmtime::Result<()> {
                let Some(op) = ControlOp::from_code(op) else {
                    bail!("unknown control op {op}");
                };
                let bridge = caller.data().bridge.clone();
                let parameter = (op == ControlOp::SetVolume).then_some(value);
                if let Err(error) = bridge.control(op, parameter) {
                    bail!("{error}");
                }
                Ok(())
            },
        )
        .map_err(map)?;

    Ok(())
}

fn read_string(caller: &mut Caller<'_, StoreData>, ptr: i32, len: i32) -> wasmtime::Result<String> {
    let (Ok(start), Ok(length)) = (usize::try_from(ptr), usize::try_from(len)) else {
        bail!("negative string pointer");
    };
    if length > 64 * 1024 {
        bail!("string argument exceeds 64 KiB");
    }
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        bail!("module lost its memory export");
    };
    let data = memory.data(&*caller);
    let end = start
        .checked_add(length)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| anyhow!("string argument outside linear memory"))?;
    std::str::from_utf8(&data[start..end])
        .map(str::to_owned)
        .map_err(|error| anyhow!("string argument is not UTF-8: {error}"))
}

/// Copy `text` into the module's memory. Returns the byte count, or the negative
/// byte count when the buffer is too small (the module can then retry).
fn write_string(
    caller: &mut Caller<'_, StoreData>,
    ptr: i32,
    cap: i32,
    text: &str,
) -> wasmtime::Result<i32> {
    let (Ok(start), Ok(capacity)) = (usize::try_from(ptr), usize::try_from(cap)) else {
        bail!("negative buffer pointer");
    };
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        bail!("module lost its memory export");
    };
    let bytes = text.as_bytes();
    let data = memory.data_mut(caller);
    if capacity < bytes.len() {
        return i32::try_from(bytes.len())
            .map(|required| -required)
            .map_err(|_| anyhow!("required length overflows i32"));
    }
    let end = start
        .checked_add(bytes.len())
        .filter(|end| *end <= data.len())
        .ok_or_else(|| anyhow!("buffer outside linear memory"))?;
    data[start..end].copy_from_slice(bytes);
    i32::try_from(bytes.len()).map_err(|_| anyhow!("length overflows i32"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::Manifest;

    const MANIFEST: &str = r#"
[plugin]
id = "io.prism.wasm"
name = "Wasm Test"
version = "1.0.0"
api = 1
entry = "main.wat"
capabilities = ["log", "notify", "player:read", "player:control"]

[[settings]]
key = "limit"
label = "Limit"
type = "integer"
default = 3
min = 0
max = 10
"#;

    fn write_plugin(wat: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("plugin.toml"), MANIFEST).expect("manifest");
        std::fs::write(dir.path().join("main.wat"), wat).expect("module");
        dir
    }

    fn runtime(dir: &Path) -> WasmRuntime {
        let manifest = Manifest::load(dir).expect("manifest");
        let engine =
            crate::audio::engine::Engine::new(crate::audio::engine::EngineConfig::default())
                .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        WasmRuntime::new(&manifest, dir, bridge).expect("runtime")
    }

    const LOGGER: &str = r#"
(module
  (import "prism" "log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (global $next (mut i32) (i32.const 256))
  (func (export "alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $ptr))
  (func (export "on_event") (param $ptr i32) (param $len i32)
    (call $log (i32.const 1) (local.get $ptr) (local.get $len))))
"#;

    #[test]
    fn a_module_receives_the_payload_it_was_allocated() {
        let dir = write_plugin(LOGGER);
        let mut runtime = runtime(dir.path());
        runtime
            .call(Hook::Event, r#"{"kind":"state_changed"}"#)
            .expect("event");
    }

    #[test]
    fn missing_hooks_are_a_no_op() {
        let dir = write_plugin(
            r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 16)))
"#,
        );
        let mut runtime = runtime(dir.path());
        runtime.call(Hook::Load, "{}").expect("load");
        runtime.call(Hook::Event, "{}").expect("event");
        runtime.call(Hook::Config, "{}").expect("config");
    }

    #[test]
    fn an_infinite_loop_exhausts_its_fuel() {
        let dir = write_plugin(
            r#"
(module
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 16))
  (func (export "on_event") (param i32 i32)
    (loop $forever (br $forever))))
"#,
        );
        let mut runtime = runtime(dir.path());
        let error = runtime.call(Hook::Event, "{}").expect_err("must abort");
        assert!(error.to_string().contains("fuel"), "{error}");
    }

    #[test]
    fn a_module_without_memory_is_rejected() {
        let dir = write_plugin(
            r#"
(module
  (func (export "alloc") (param i32) (result i32) (i32.const 16)))
"#,
        );
        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine =
            crate::audio::engine::Engine::new(crate::audio::engine::EngineConfig::default())
                .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        let error = WasmRuntime::new(&manifest, dir.path(), bridge).expect_err("must reject");
        assert!(error.to_string().contains("memory"), "{error}");
    }

    #[test]
    fn an_unlisted_import_fails_to_link() {
        let dir = write_plugin(
            r#"
(module
  (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 16)))
"#,
        );
        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine =
            crate::audio::engine::Engine::new(crate::audio::engine::EngineConfig::default())
                .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        let error = WasmRuntime::new(&manifest, dir.path(), bridge).expect_err("must reject");
        assert!(
            error.to_string().contains("instantiate")
                || error.to_string().contains("unknown import"),
            "{error}"
        );
    }

    #[test]
    fn a_greedy_memory_request_is_capped() {
        // 2000 pages = 125 MiB, above the 64 MiB ceiling.
        let dir = write_plugin(
            r#"
(module
  (memory (export "memory") 2000)
  (func (export "alloc") (param i32) (result i32) (i32.const 16)))
"#,
        );
        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine =
            crate::audio::engine::Engine::new(crate::audio::engine::EngineConfig::default())
                .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        let error = WasmRuntime::new(&manifest, dir.path(), bridge).expect_err("must reject");
        assert!(
            error.to_string().to_lowercase().contains("memory")
                || error.to_string().contains("limit"),
            "{error}"
        );
    }

    #[test]
    fn control_requires_the_declared_capability() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("plugin.toml"),
            MANIFEST.replace(
                r#"capabilities = ["log", "notify", "player:read", "player:control"]"#,
                r#"capabilities = ["log"]"#,
            ),
        )
        .expect("manifest");
        std::fs::write(
            dir.path().join("main.wat"),
            r#"
(module
  (import "prism" "control" (func $control (param i32 f64)))
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 16))
  (func (export "on_event") (param i32 i32)
    (call $control (i32.const 0) (f64.const 0))))
"#,
        )
        .expect("module");

        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine =
            crate::audio::engine::Engine::new(crate::audio::engine::EngineConfig::default())
                .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        let mut runtime = WasmRuntime::new(&manifest, dir.path(), bridge).expect("runtime");
        let error = runtime.call(Hook::Event, "{}").expect_err("must deny");
        assert!(error.to_string().contains("not allowed"), "{error}");
    }
}
