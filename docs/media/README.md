# Media

Screenshots in this folder are captured from the running application, never mocked.

## Capturing on Linux (headless CI or workstation)

```bash
sudo apt-get install -y xvfb imagemagick dbus-x11
./scripts/capture-screenshots.sh
```

The script starts a private X server (`1440x900x24`), launches the Tauri shell inside a D-Bus
session, waits for the WebView to paint the diagnostics screen, and writes:

- `overview.png` — shell, ambient canvas, live library counters and engine status.

Planned captures for later phases: modular layout editor, parametric EQ, lyrics panel, Discord RPC
card, and a 60-second fast-scroll recording exported as GIF at 60 FPS.

- `home-fresh-install.png` — the **0.1.1** release binary on a pristine data directory: Home, the
  empty-state card, the counter row and the "Recently added" empty state. Captured under Xvfb with
  `WEBKIT_DISABLE_COMPOSITING_MODE=1`; this is the frame that used to be a black window.

The v0.1.1 release was verified the same way: a `--features tauri/custom-protocol` build, a fresh
`XDG_*` root, and a screenshot of the window — not a browser tab.
