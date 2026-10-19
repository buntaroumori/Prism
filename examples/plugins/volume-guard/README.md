# Volume Guard (example plugin)

A complete, working plugin that demonstrates the whole Lua host contract:
manifest validation, declared capabilities, typed settings, the three hooks and
the `prism` API.

```
examples/plugins/volume-guard/
├── plugin.toml   # manifest: identity, capabilities, settings schema
├── main.lua      # the plugin
└── README.md
```

## Install

1. Build with the Lua runtime: `cargo tauri dev --features lua-plugins`, or
   `cargo build --features lua-plugins`.
2. Open **Settings → Plugins → Folder** and copy this directory into it.
3. Press **Rescan**, then enable _Volume Guard_.

## What it does

On every `track_changed` event it applies `target_db` through
`prism.set_volume`, and optionally announces the change with
`prism.emit`. Both settings are validated twice: once by the manifest schema
(rejects out-of-range values before they are stored) and once by the plugin
before it calls the host.

## What it cannot do

The sandbox removes `io`, `os`, `package`, `debug`, `dofile`, `loadfile`,
`load`, `require` and `print`. A plugin cannot read a file, spawn a process,
open a socket or reach the network; it receives only the host functions its
manifest declares, and a call without the matching capability fails with a
permission error.
