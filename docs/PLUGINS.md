# Plugin development

Prism supports optional Lua and WebAssembly plugins. The plugin API is versioned, and each plugin
must declare the host capabilities it uses. The default application build includes neither runtime.

## Runtime features

| Cargo feature  | Runtime                        |
| :------------- | :----------------------------- |
| `lua-plugins`  | Lua 5.4 through `mlua`         |
| `wasm-plugins` | WebAssembly through `wasmtime` |

For a development build with Lua support:

```bash
pnpm tauri dev -- --features lua-plugins
```

For a WebAssembly build, replace the feature with `wasm-plugins`. Multiple features can be supplied
through the corresponding Cargo feature syntax.

Without the required feature, Prism can still discover and validate a manifest, but the plugin
descriptor reports that its runtime is unavailable.

## Plugin directory

A plugin is a directory in the Prism plugin folder containing a manifest and its entry file:

```text
my-plugin/
├── plugin.toml
└── main.lua
```

**Settings → Plugins → Folder** opens the plugin directory. After copying a plugin, use **Rescan**
and enable it from the Plugins panel.

## Manifest

```toml
[plugin]
id = "io.example.volume-guard"
name = "Volume Guard"
version = "1.0.0"
api = 1
entry = "main.lua"
description = "Applies a configured listening level on track changes."
author = "Example Author"
capabilities = ["log", "notify", "player:read", "player:control"]

[[settings]]
key = "target_db"
label = "Target level"
type = "number"
default = -12.0
min = -40.0
max = 0.0
help = "Output level in decibels."

[[settings]]
key = "announce"
label = "Announce changes"
type = "boolean"
default = false
```

The entry extension selects the runtime: `.lua` for Lua, or `.wasm`/`.wat` for WebAssembly.

Manifest validation checks:

- reverse-DNS style identifiers, semantic versions and the supported API revision;
- unknown keys and duplicate settings;
- entry paths that must remain inside the plugin directory;
- declared capabilities;
- setting definitions and their default values.

An invalid manifest is rejected with an error in the Plugins panel.

## Settings

Setting controls support `string`, `number`, `integer`, `boolean` and `select` types. Numeric and
integer settings can define ranges, and select settings define their available choices.

Configuration is stored per plugin and validated when loaded or changed. Lua plugins can read a
setting as a native Lua value:

```lua
local target = prism.setting("target_db")
local enabled = prism.setting("announce") == true
```

A missing setting returns `nil`.

## Capabilities

| Capability       | Host surface                                         |
| :--------------- | :--------------------------------------------------- |
| `log`            | Write a plugin message through `prism.log`           |
| `notify`         | Emit a user-facing notification through `prism.emit` |
| `player:read`    | Read the current playback status                     |
| `player:control` | Pause, resume or change volume                       |

Calling a host function without its declared capability returns a permission error. Removing an
unused capability from the manifest reduces the plugin's available host surface.

## Lua API

A Lua plugin runs once at load and may define these optional globals:

```lua
function on_load(config_json)
end

function on_event(event_json)
end

function on_config(config_json)
end
```

The host table provides:

| Function                     | Description                               |
| :--------------------------- | :---------------------------------------- |
| `prism.setting(key)`         | Return a configured value or `nil`        |
| `prism.log(level, message)`  | Write a plugin log message                |
| `prism.emit(topic, message)` | Emit a notification                       |
| `prism.now_playing()`        | Return current playback information       |
| `prism.pause()`              | Pause playback                            |
| `prism.resume()`             | Resume playback                           |
| `prism.set_volume(db)`       | Set volume in the supported decibel range |

Event payloads are JSON strings. The sandbox does not include a JSON decoder, so simple event
routing can match the event kind while more complex processing requires plugin-provided parsing.

The standard Lua environment is restricted. Filesystem, operating-system, package-loading and debug
facilities are not exposed.

## WebAssembly ABI

WebAssembly plugins exchange UTF-8 JSON through linear memory. The host imports are:

```text
log(level: i32, ptr: i32, len: i32)
emit(ptr: i32, len: i32)
now_playing(ptr: i32, cap: i32) -> i32
config(ptr: i32, cap: i32) -> i32
control(op: i32, value: f64)
```

The module exports `memory`, `alloc`, and the callbacks it implements:

```text
alloc(len: i32) -> i32
on_load(ptr: i32, len: i32)       optional
on_event(ptr: i32, len: i32)      required for event delivery
on_config(ptr: i32, len: i32)     optional
```

Buffer reads are bounds-checked and must contain valid UTF-8. Read-style host calls return the
number of bytes written or a negative value indicating the required capacity. WebAssembly modules
run without WASI and can only resolve the declared Prism imports.

## Events and failure handling

The plugin bridge forwards playback events such as state changes, track changes, skipped tracks,
device changes, output errors and underruns.

Each plugin has its own runtime instance and execution budgets. A runtime exception, WebAssembly
trap, permission failure or exhausted budget disables the plugin for the current session and
records the reason in its descriptor. Playback continues independently.

Configuration is stored in the application settings table under the plugin's identity. Disabled
plugin ids and configuration survive application restarts; runtime failures themselves are scoped
to the session.

## Example

[Volume Guard](../examples/plugins/volume-guard/README.md) demonstrates a Lua manifest, typed
settings, all three lifecycle hooks, event handling and capability-gated playback control. Its
source is intentionally small enough to use as a starting point for another Lua plugin.
