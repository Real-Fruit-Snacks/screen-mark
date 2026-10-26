# screen-mark

Screenshot capture and annotation in pure Rust

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/cover-dark.svg">
  <img alt="screen-mark capture overlay: a dimmed desktop, an Ember-bordered selection, an arrow, a box, and a step marker, with the tool bar beneath" src="docs/assets/cover-light.svg" width="100%">
</picture>

Windows: download `screen-mark-vX.Y.Z-windows-x64.msi` (installer) or `.zip` (portable) from [Releases](https://github.com/Real-Fruit-Snacks/screen-mark/releases/latest). Linux: `screen-mark-vX.Y.Z-linux-x64.tar.gz` from the same page. Or build it:

```sh
cargo install --git https://github.com/Real-Fruit-Snacks/screen-mark --locked
```

I wanted a screenshot tool that starts in under a second, draws an arrow without a round trip through a web view, and leaves no background service I didn't ask for. This is that: `winit` for windows, `tiny-skia` for drawing, a tray icon, one global hotkey, and a TOML file. It assumes Windows 10 or later; the Linux build captures under X11 and nothing else yet.

## What's in it

| Command | Does |
|---|---|
| `screen-mark` | Starts in the tray; `Ctrl+Shift+S` opens the capture overlay |
| `screen-mark capture --clipboard` | Captures the full screen to the clipboard, no UI |
| `screen-mark capture --save out.png` | Captures the full screen to a file, no UI |
| `screen-mark capture --delay 5` | Waits five seconds, then opens the overlay; add a flag above to skip it |
| `screen-mark install` / `uninstall` | Copies the exe to AppData with a Start Menu entry and PATH; the portable exe asks before doing this |

In the overlay: drag for a region, click for a window, `Escape` cancels. `Enter` or `Ctrl+C` copies the selection with annotations, `Ctrl+S` saves, `Ctrl+Z` / `Ctrl+Shift+Z` undo and redo, scroll wheel changes tool size. From the toolbar: pin the capture as an always-on-top window, upload it anonymously to Imgur (needs your own client ID, see Configuration), or run Windows OCR on it and copy the text. The last 20 captures are kept in a History window.

Fourteen annotation tools, one key each:

| Key | Tool | Key | Tool |
|---|---|---|---|
| `V` | Select / move | `H` | Highlight (multiply blend) |
| `A` | Arrow | `F` | Spotlight |
| `R` | Rectangle | `T` | Text (`Ctrl+V` pastes) |
| `C` | Circle | `B` | Pixelate |
| `O` | Rounded rectangle | `N` | Step marker, auto-numbered |
| `L` | Line | `I` | Eyedropper |
| `P` | Pencil | `M` | Measurement |

Dark and light modes, switched in Settings › General or with `theme` in the config file. Five annotation color presets (red, blue, green, yellow, mauve) or any `#rrggbb`.

## Tested on

- Windows 11 · Rust stable — my daily machine, every release used in anger before tagging
- Windows Server 2025 · Rust stable — CI (`windows-latest`) builds, tests, and runs clippy on every push
- Ubuntu 24.04 · X11 · Rust stable — CI (`ubuntu-latest`) builds and tests; region capture checked by hand
- *Not* tested: Wayland — the backend is a stub and capture does nothing. macOS — does not build.

## Known issues

- Upgrading from HydroShot 1.x: config, history, and the autostart entry are migrated on first run. The old self-installed copy is not removed; run `hydroshot uninstall` once, or let the 2.0 MSI replace the 1.x MSI.
- Linux capture is X11 only and experimental; multi-monitor and HiDPI are not verified there.
- OCR is Windows only. It shells out to PowerShell and the Windows.Media.Ocr API, so the OCR language pack for your display language has to be installed.
- Imgur upload ships with no client ID. Register one at api.imgur.com and set it in Settings or `SCREEN_MARK_IMGUR_CLIENT_ID`.
- Window capture skips cloaked UWP windows on purpose; some Store apps will not be selectable by click.
- CLI `--delay` waits silently. The on-screen countdown only appears for tray-menu delays.

## Configuration

Written on first run to `%APPDATA%\screen-mark\config.toml` (Windows) or `~/.config/screen-mark/config.toml` (Linux). Every key is optional; missing keys take the defaults below.

```toml
[general]
default_color = "blue"        # red | blue | green | yellow | mauve | "#rrggbb"
default_thickness = 3.0
save_directory = ""           # set a path to save silently; empty opens a PNG/JPEG/WebP dialog
imgur_client_id = ""          # or SCREEN_MARK_IMGUR_CLIENT_ID in the environment
history_enabled = true
theme = "dark"                # dark | light

[hotkey]
capture = "Ctrl+Shift+S"      # also rebindable in Settings > General

[shortcuts]                   # one key per tool; defaults are the table above
select = "v"

[toolbar]                     # hide tools you never use
measurement = false
```

Source layout, for the curious: `src/capture/` has one backend per platform, `src/tools/` has one file per annotation tool, `src/overlay/` is the selection UI, and `src/state.rs` holds the annotation command stack that undo and redo replay. `docs/` is the [website](https://real-fruit-snacks.github.io/screen-mark/), deployed by `pages.yml` on every push to `main`; `.gitlab-ci.yml` does the same on GitLab for air-gapped hosting.

## License

MIT
