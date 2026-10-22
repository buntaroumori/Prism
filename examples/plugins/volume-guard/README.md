# Volume Guard

Volume Guard is a small Lua plugin that applies a configured output level whenever the track
changes. It demonstrates manifest settings, lifecycle hooks, playback events and capability-gated
host calls.

## Requirements

Prism must be built with the `lua-plugins` feature:

```bash
pnpm tauri dev -- --features lua-plugins
```

The default build does not include the Lua runtime.

## Installation

1. Copy the `volume-guard` directory into the Prism plugin folder.
2. Open **Settings → Plugins** and select **Rescan**.
3. Enable **Volume Guard**.
4. Configure the target level and optional notifications.

**Settings → Plugins → Folder** opens the plugin directory on the current system.

## Settings

| Setting     | Type    | Default | Description                                       |
| :---------- | :------ | :------ | :------------------------------------------------ |
| `target_db` | Number  | `-12.0` | Level applied on a track change, from −40 to 0 dB |
| `announce`  | Boolean | `false` | Emit a notification after applying the level      |

The manifest constrains both settings. The plugin also validates the target before calling the
host.

## Implementation

- `on_load` reads the saved configuration and logs the initial level.
- `on_config` reads updated settings after a configuration change.
- `on_event` listens for `track_changed` and applies the configured level.
- `prism.set_volume` performs the volume change through `player:control`.
- `prism.log` and `prism.emit` use their respective declared capabilities.

The event callback receives a JSON string. This example only matches the event kind and therefore
does not require a JSON decoder.

## Sandbox

The plugin cannot directly access files, processes, sockets or the network. Its available host
functions are limited by the capabilities declared in `plugin.toml`. Runtime errors and exhausted
resource budgets disable the plugin for the session without stopping playback.

See [Plugin development](../../../docs/PLUGINS.md) for the complete manifest and host API reference.
