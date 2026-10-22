# Media assets

These assets support the project documentation.

## Application screenshots

| File              | View                                           |
| :---------------- | :--------------------------------------------- |
| `home.png`        | Home with genre-based album shelves            |
| `library.png`     | Library album grid, sorting and genre filter   |
| `now-playing.png` | Now Playing with artwork and spectrum analysis |
| `appearance.png`  | Appearance settings                            |

The screenshots are referenced by the [repository README](../../README.md#screenshots). Keep their
filenames stable when replacing them so documentation links remain valid.

## Profiling diagrams

| File                      | Workload                                                   |
| :------------------------ | :--------------------------------------------------------- |
| `flamegraph-decode.svg`   | Decode and DSP processing without a sample-rate conversion |
| `flamegraph-resample.svg` | Decode, DSP and resampling                                 |

See [Performance and validation](../PERFORMANCE.md#cpu-profiling) for the commands used to profile
these paths.

## Local capture tool

`scripts/capture-screenshots.sh` starts the Tauri application under Xvfb on Linux and writes an
`overview.png` capture. It requires Xvfb, ImageMagick and D-Bus:

```bash
./scripts/capture-screenshots.sh
```

The capture script is a development utility; the curated screenshots above are maintained as
documentation assets.
