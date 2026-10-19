//! Lua 5.4 sandbox (`lua-plugins` feature).
//!
//! One `Lua` state per plugin, created with a restricted standard library:
//! `table`, `string`, `math`, `utf8` and `coroutine` are available, the base
//! library is stripped of `dofile`/`loadfile`/`load`/`print`, and `io`, `os`,
//! `package`, `require` and `debug` are never exposed. A plugin cannot open a
//! file, spawn a process or reach the network — it can only call the host table
//! it was handed.
//!
//! Two budgets apply: 16 MiB of Lua memory and an instruction ceiling checked
//! every 10 000 VM instructions. Exceeding either raises a normal Lua error
//! that propagates as a plugin failure instead of hanging the dispatcher.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mlua::{Function, HookTriggers, Lua, LuaOptions, RegistryKey, StdLib, Value as LuaValue, VmState};

use super::manifest::Manifest;
use super::{ControlOp, Hook, HostBridge, LUA_INSTRUCTION_BUDGET, LUA_MEMORY_LIMIT, PluginError};

/// Largest accepted entry file. A plugin is glue, not an application.
const MAX_SOURCE: usize = 512 * 1024;
/// How often the instruction hook runs.
const HOOK_STRIDE: u32 = 10_000;

pub(crate) struct LuaRuntime {
    lua: Lua,
    #[allow(dead_code)]
    bridge: Arc<HostBridge>,
    on_load: Option<RegistryKey>,
    on_event: Option<RegistryKey>,
    on_config: Option<RegistryKey>,
    budget: Arc<AtomicU64>,
}

impl std::fmt::Debug for LuaRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LuaRuntime")
            .field("on_load", &self.on_load.is_some())
            .field("on_event", &self.on_event.is_some())
            .field("on_config", &self.on_config.is_some())
            .finish_non_exhaustive()
    }
}

impl LuaRuntime {
    pub(crate) fn new(
        manifest: &Manifest,
        dir: &Path,
        bridge: Arc<HostBridge>,
    ) -> Result<Self, PluginError> {
        let path = dir.join(&manifest.entry);
        let source = std::fs::read_to_string(&path)
            .map_err(|error| PluginError::io(path.clone(), error))?;
        if source.len() > MAX_SOURCE {
            return Err(PluginError::manifest(format!(
                "{} exceeds {} KiB",
                path.display(),
                MAX_SOURCE / 1024
            )));
        }

        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
        let lua = Lua::new_with(libs, LuaOptions::default())
            .map_err(|error| PluginError::runtime(format!("cannot create Lua state: {error}")))?;

        if let Err(error) = lua.set_memory_limit(LUA_MEMORY_LIMIT) {
            // Without the custom allocator the ceiling is advisory; the
            // instruction budget still bounds wall-clock time.
            tracing::warn!(plugin = %manifest.id, %error, "Lua memory limit unavailable");
        }

        let budget = Arc::new(AtomicU64::new(0));
        let hook_budget = Arc::clone(&budget);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_STRIDE),
            move |_lua, _debug| {
                let used = hook_budget.fetch_add(u64::from(HOOK_STRIDE), Ordering::Relaxed);
                if used >= LUA_INSTRUCTION_BUDGET {
                    return Err(mlua::Error::runtime(format!(
                        "instruction budget of {LUA_INSTRUCTION_BUDGET} exceeded"
                    )));
                }
                Ok(VmState::Continue)
            },
        );

        let globals = lua.globals();
        for name in [
            "dofile",
            "loadfile",
            "load",
            "require",
            "print",
            "io",
            "os",
            "package",
            "debug",
            "collectgarbage",
        ] {
            globals
                .set(name, LuaValue::Nil)
                .map_err(|error| PluginError::runtime(format!("cannot seal global '{name}': {error}")))?;
        }

        let api = lua
            .create_table()
            .map_err(|error| PluginError::runtime(format!("cannot create host table: {error}")))?;
        register_api(&lua, &api, &bridge, manifest)?;
        globals
            .set("prism", api)
            .map_err(|error| PluginError::runtime(format!("cannot publish host table: {error}")))?;
        globals
            .set("PLUGIN_ID", manifest.id.as_str())
            .map_err(|error| PluginError::runtime(format!("cannot publish plugin id: {error}")))?;
        globals
            .set("PLUGIN_VERSION", manifest.version.as_str())
            .map_err(|error| PluginError::runtime(format!("cannot publish version: {error}")))?;

        let chunk_name = format!("@{}", manifest.entry.display());
        lua.load(&source)
            .set_name(chunk_name)
            .exec()
            .map_err(|error| {
                PluginError::runtime(format!("{} failed to load: {error}", manifest.id))
            })?;

        let on_load = take_hook(&lua, &globals, "on_load")?;
        let on_event = take_hook(&lua, &globals, "on_event")?;
        let on_config = take_hook(&lua, &globals, "on_config")?;

        Ok(Self {
            lua,
            bridge,
            on_load,
            on_event,
            on_config,
            budget,
        })
    }

    /// Invoke one hook. Missing hooks are a no-op: a plugin only implements the
    /// entry points it cares about.
    pub(crate) fn call(&mut self, hook: Hook, payload: &str) -> Result<(), PluginError> {
        let key = match hook {
            Hook::Load => &self.on_load,
            Hook::Event => &self.on_event,
            Hook::Config => &self.on_config,
        };
        let Some(key) = key else {
            return Ok(());
        };

        // Every call gets a fresh budget, so one expensive hook cannot starve
        // the next event.
        self.budget.store(0, Ordering::Relaxed);

        let function: Function = self.lua.registry_value(key).map_err(|error| {
            PluginError::runtime(format!("cannot resolve {}: {error}", hook.as_str()))
        })?;
        let _: () = function.call(payload).map_err(|error| {
            PluginError::runtime(format!("{} handler failed: {error}", hook.as_str()))
        })?;
        Ok(())
    }
}

fn take_hook(lua: &Lua, globals: &mlua::Table, name: &str) -> Result<Option<RegistryKey>, PluginError> {
    let value: LuaValue = globals
        .get(name)
        .map_err(|error| PluginError::runtime(format!("cannot read '{name}': {error}")))?;
    match value {
        LuaValue::Nil => Ok(None),
        LuaValue::Function(function) => lua
            .create_registry_value(function)
            .map(Some)
            .map_err(|error| PluginError::runtime(format!("cannot retain '{name}': {error}"))),
        _ => Err(PluginError::manifest(format!(
            "'{name}' must be a function, found a {}",
            value.type_name()
        ))),
    }
}

fn runtime_error(error: PluginError) -> mlua::Error {
    mlua::Error::RuntimeError(error.to_string())
}

/// Build the `prism` host table.
fn register_api(
    lua: &Lua,
    api: &mlua::Table,
    bridge: &Arc<HostBridge>,
    manifest: &Manifest,
) -> Result<(), PluginError> {
    let to_error = |error: mlua::Error| PluginError::runtime(format!("cannot register host api: {error}"));

    let log_bridge = Arc::clone(bridge);
    let log = lua
        .create_function(move |_, (level, message): (String, String)| {
            log_bridge.log(&level, &message).map_err(runtime_error)
        })
        .map_err(to_error)?;
    api.set("log", log).map_err(to_error)?;

    let emit_bridge = Arc::clone(bridge);
    let emit = lua
        .create_function(move |_, (topic, message): (Option<String>, String)| {
            emit_bridge.notify(topic, &message).map_err(runtime_error)
        })
        .map_err(to_error)?;
    api.set("emit", emit).map_err(to_error)?;

    let status_bridge = Arc::clone(bridge);
    let now_playing = lua
        .create_function(move |_, ()| status_bridge.now_playing().map_err(runtime_error))
        .map_err(to_error)?;
    api.set("now_playing", now_playing).map_err(to_error)?;

    let config_bridge = Arc::clone(bridge);
    let config = lua
        .create_function(move |_, ()| Ok(config_bridge.config_json()))
        .map_err(to_error)?;
    api.set("config", config).map_err(to_error)?;

    let pause_bridge = Arc::clone(bridge);
    let pause = lua
        .create_function(move |_, ()| {
            pause_bridge
                .control(ControlOp::Pause, None)
                .map_err(runtime_error)
        })
        .map_err(to_error)?;
    api.set("pause", pause).map_err(to_error)?;

    let resume_bridge = Arc::clone(bridge);
    let resume = lua
        .create_function(move |_, ()| {
            resume_bridge
                .control(ControlOp::Resume, None)
                .map_err(runtime_error)
        })
        .map_err(to_error)?;
    api.set("resume", resume).map_err(to_error)?;

    let volume_bridge = Arc::clone(bridge);
    let set_volume = lua
        .create_function(move |_, db: f64| {
            volume_bridge
                .control(ControlOp::SetVolume, Some(db))
                .map_err(runtime_error)
        })
        .map_err(to_error)?;
    api.set("set_volume", set_volume).map_err(to_error)?;

    api.set("api", manifest.api).map_err(to_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::Manifest;
    use crate::plugins::PluginError as Error;

    fn write_plugin(source: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("plugin.toml"),
            r#"
[plugin]
id = "io.prism.test"
name = "Test"
version = "1.0.0"
api = 1
entry = "main.lua"
capabilities = ["log", "notify", "player:read", "player:control"]

[[settings]]
key = "limit"
label = "Limit"
type = "integer"
default = 3
min = 0
max = 10
"#,
        )
        .expect("manifest");
        std::fs::write(dir.path().join("main.lua"), source).expect("entry");
        dir
    }

    fn runtime(dir: &Path) -> LuaRuntime {
        let manifest = Manifest::load(dir).expect("manifest");
        let engine = crate::audio::engine::Engine::new(
            crate::audio::engine::EngineConfig::default(),
        )
        .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(
            &manifest,
            manifest.defaults(),
            engine,
            messages,
        ));
        LuaRuntime::new(&manifest, dir, bridge).expect("runtime")
    }

    #[test]
    fn a_plugin_runs_and_hooks_fire() {
        let dir = write_plugin(
            r#"
local seen = 0
function on_load(config)
  seen = 1
  prism.log("info", "loaded with " .. config)
end
function on_event(event)
  seen = seen + 1
end
function on_config(config)
  prism.log("debug", "config changed")
end
"#,
        );
        let mut runtime = runtime(dir.path());
        runtime.call(Hook::Load, "{\"limit\":3}").expect("on_load");
        runtime.call(Hook::Event, "{\"kind\":\"state_changed\"}").expect("on_event");
        runtime.call(Hook::Config, "{}").expect("on_config");
        runtime.call(Hook::Event, "{}").expect("second event");
    }

    #[test]
    fn missing_hooks_are_a_no_op() {
        let dir = write_plugin("local value = 1\n");
        let mut runtime = runtime(dir.path());
        runtime.call(Hook::Load, "{}").expect("no on_load");
        runtime.call(Hook::Event, "{}").expect("no on_event");
        runtime.call(Hook::Config, "{}").expect("no on_config");
    }

    #[test]
    fn filesystem_and_process_globals_are_sealed() {
        let dir = write_plugin(
            r#"
function on_load()
  assert(io == nil, "io must be sealed")
  assert(os == nil, "os must be sealed")
  assert(dofile == nil, "dofile must be sealed")
  assert(loadfile == nil, "loadfile must be sealed")
  assert(load == nil, "load must be sealed")
  assert(require == nil, "require must be sealed")
  assert(package == nil, "package must be sealed")
  assert(debug == nil, "debug must be sealed")
  assert(type(table.concat) == "function", "table must stay available")
  assert(type(string.format) == "function", "string must stay available")
  assert(type(math.floor) == "function", "math must stay available")
end
"#,
        );
        let mut runtime = runtime(dir.path());
        runtime.call(Hook::Load, "{}").expect("sandbox verified in Lua");
    }

    #[test]
    fn a_runtime_error_propagates_and_the_next_call_still_works() {
        let dir = write_plugin(
            r#"
function on_event(event)
  if string.find(event, "boom", 1, true) then
    error("deliberate")
  end
end
"#,
        );
        let mut runtime = runtime(dir.path());
        let error = runtime.call(Hook::Event, "boom").expect_err("must fail");
        assert!(error.to_string().contains("deliberate"), "{error}");
        runtime.call(Hook::Event, "fine").expect("recovers");
    }

    #[test]
    fn an_infinite_loop_hits_the_instruction_budget() {
        let dir = write_plugin("function on_event()\n  while true do end\nend\n");
        let mut runtime = runtime(dir.path());
        let error = runtime.call(Hook::Event, "{}").expect_err("must abort");
        assert!(error.to_string().contains("budget"), "{error}");
    }

    #[test]
    fn a_non_function_hook_is_rejected_at_load() {
        let dir = write_plugin("on_event = 42\n");
        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine = crate::audio::engine::Engine::new(
            crate::audio::engine::EngineConfig::default(),
        )
        .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(&manifest, manifest.defaults(), engine, messages));
        let error = LuaRuntime::new(&manifest, dir.path(), bridge).expect_err("must reject");
        let Error::Manifest(message) = error else {
            panic!("expected a manifest error, got {error:?}");
        };
        assert!(message.contains("on_event"));
    }

    #[test]
    fn an_undeclared_capability_is_refused_at_call_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("plugin.toml"),
            r#"
[plugin]
id = "io.prism.restricted"
name = "Restricted"
version = "1.0.0"
api = 1
entry = "main.lua"
capabilities = ["log"]
"#,
        )
        .expect("manifest");
        std::fs::write(
            dir.path().join("main.lua"),
            "function on_event() prism.pause() end\n",
        )
        .expect("entry");

        let manifest = Manifest::load(dir.path()).expect("manifest");
        let engine = crate::audio::engine::Engine::new(
            crate::audio::engine::EngineConfig::default(),
        )
        .expect("engine");
        let (messages, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = Arc::new(HostBridge::new(&manifest, manifest.defaults(), engine, messages));
        let mut runtime = LuaRuntime::new(&manifest, dir.path(), bridge).expect("runtime");
        let error = runtime.call(Hook::Event, "{}").expect_err("must deny");
        assert!(error.to_string().contains("not allowed"), "{error}");
    }
}
