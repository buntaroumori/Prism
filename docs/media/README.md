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
