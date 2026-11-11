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

I wanted a screenshot tool that starts in under a second, draws an arrow without a round trip through a web view, and leaves no background service I didn't ask for. This is that: `winit` for windows, `tiny-skia` for drawing, a tray icon, three global hotkeys, and a TOML file. It assumes Windows 10 or later; the Linux build captures under X11 and nothing else yet.

## What's in it

| Command | Does |
|---|---|
| `screen-mark` | Starts in the tray. `Ctrl+PrintScreen` saves a PNG of the monitor under the mouse and copies it, in one press. `Ctrl+Shift+S` opens the capture overlay. `Shift+PrintScreen` captures the last region again |
| `screen-mark capture --clipboard` | Captures the full screen to the clipboard, no UI |
| `screen-mark capture --save out.png` | Captures the full screen to a file, no UI |
| `screen-mark capture --delay 5` | Waits five seconds, then opens the overlay; add a flag above to skip it |
| `screen-mark install` / `uninstall` | Copies the exe to AppData with a Start Menu entry and PATH; the portable exe asks before doing this |
| `screen-mark update` | Looks for a newer release and installs it; `--check` only looks. See [Updating](#updating) |

The one-press screenshot saves to `Pictures\screen-mark` (or the folder you pick in Settings › Capture) under a timestamped name that never overwrites, then puts the same image on the clipboard. `Shift+PrintScreen` does the same for the area of the last region you captured, so a window or a chart you capture every day is one key; screen-mark remembers that area across restarts. A small popup in the corner says what happened; click it to open the folder with the file selected. Settings › Capture can switch the popup to system notifications or turn it off; errors always show.

In the overlay: drag for a region, click for a window, `Escape` cancels. `Enter` or `Ctrl+C` copies the selection with annotations, `Ctrl+S` saves, `Ctrl+Z` / `Ctrl+Shift+Z` undo and redo, scroll wheel changes tool size. From the toolbar: pin the capture as an always-on-top window, upload it anonymously to Imgur (needs your own client ID, see Configuration), or run Windows OCR on it and copy the text. The last 20 captures are kept in a History window. Two switches in Settings › Capture change the region flow: *Save a file every time I copy* writes a PNG alongside each copy, and *Copy as soon as a region is drawn* skips the markup toolbar so drag-and-release is the whole job.

For pixel-exact work a magnifier follows the pointer while you place the region, and the arrow keys move the pointer one pixel at a time (ten with `Shift`). Once a region exists the arrows move it and `Ctrl`+arrows resize it; with an annotation selected they move that instead. Hold `Shift` while dragging for a square region, squares, circles, and lines at 15° steps.

Fifteen annotation tools, one key each:

| Key | Tool | Key | Tool |
|---|---|---|---|
| `V` | Select / move | `H` | Highlight (multiply blend) |
| `A` | Arrow | `F` | Spotlight |
| `R` | Rectangle | `T` | Text (`Shift+Enter` new line, `Ctrl+V` pastes) |
| `C` | Circle | `B` | Pixelate |
| `O` | Rounded rectangle | `N` | Step marker, auto-numbered |
| `L` | Line | `I` | Eyedropper |
| `P` | Pencil | `M` | Measurement |
| `X` | Redact (solid box) | | |

Redact is the one for passwords and addresses: Pixelate keeps an average of what is under it, Redact keeps nothing. While typing text, `Enter` finishes, the arrow keys, `Home` and `End` move the caret, and a click anywhere else finishes the text and carries on. Characters the built-in font lacks (Chinese, Japanese, Korean and so on) are drawn with a font from the system.

Every key above is rebindable in Settings › Keys: click a binding, press the new combination. `Esc` cancels, `Backspace` leaves it unbound. Settings refuses a combination another binding already uses, one Windows keeps for itself (`Alt+Tab`, `Win+L`, ...), or a global hotkey another app has registered, and says which. A global hotkey with a single modifier (`Alt+S`) or a bare F-key is allowed, with a note that other apps won't see it while screen-mark runs. Pin, upload, and OCR start unbound and can be given keys there too.

Settings and History work without a mouse: `Tab` or the arrow keys move between controls, `Enter` or `Space` presses one, `Ctrl+Tab` changes tab, `Ctrl+Enter` saves. On Windows both windows are described to screen readers.

Dark and light modes, switched in Settings › General or with `theme` in the config file. Five annotation color presets (red, blue, green, yellow, mauve) or any `#rrggbb`.

## Updating

screen-mark contacts nothing unless you ask it to. *Check for updates* in the tray menu, or `screen-mark update --check`, asks GitHub for the newest release. If there is one, the same menu item becomes *Install screen-mark X.Y.Z*: it downloads the release, puts it in place and restarts. `screen-mark update` does the same from a terminal.

*Check for a new version once a day* in Settings › General (`auto_check` in the config) looks a minute after screen-mark starts and once a day after that. It tells you when there is a new version and installs nothing by itself.

What gets installed is checked four ways:

- The release's `latest.json` has to carry an Ed25519 signature made with one of two keys compiled into screen-mark (`src/update/keys.rs`). Where it was downloaded from doesn't count for anything.
- Each download has to have the size and SHA-256 that signed file gives.
- The new program is run once, to see that it starts and is the version the release says, before it replaces the old one.
- A release that isn't newer than the running one is never installed.

The version that was replaced stays beside the new one, as `screen-mark.old.exe`, until the new one has started.

How a copy updates depends on how it was installed. One screen-mark installed itself, or one unpacked from the zip or tar.gz, has its program file replaced. One installed with the MSI runs the new MSI, so Windows' list of installed apps stays right. A copy from Scoop, winget or Chocolatey, or in a folder screen-mark can't write to, is left alone: screen-mark says there is a new version and leaves installing it to the package manager, or to you.

To update from somewhere other than GitHub, set `url` under `[updates]` to a web folder or a folder path that holds `latest.json`, `latest.json.sig` and the release's downloads. The `docs/downloads/` folder of the website zip is one. A folder or share can also be picked with *Folder* next to *From* in Settings › General, and *GitHub* goes back. The signature is checked the same way, so plain `http://` or a file share is fine. `screen-mark update --from <folder or URL>` updates from somewhere else once, without changing the setting.

Behind a proxy, updates use the one in `HTTPS_PROXY` / `HTTP_PROXY` (with `NO_PROXY`) or, on Windows, the one in Settings › Network & internet › Proxy. A proxy that a setup script (PAC) or automatic detection provides isn't read: put it in `proxy` under `[updates]` (`proxy = "http://proxy.example:8080"`), or `proxy = "none"` to go direct. Connections to GitHub are checked against the certificates built into screen-mark, not the ones Windows trusts, so a network that inspects HTTPS breaks them; on one of those, serve updates from an internal `http://` folder or a file share instead.

## Tested on

- Windows 11 · Rust stable — my daily machine, every release used in anger before tagging
- Windows Server 2025 · Rust stable — CI (`windows-latest`) builds, tests, and runs clippy on every push
- Ubuntu 24.04 · X11 · Rust stable — CI (`ubuntu-latest`) builds and tests, then drives the real binary on a virtual X display with two monitors (`tests/e2e/`): hotkeys, popups, rebinding, the tray menu, unwritable folders, system notifications
- *Not* tested: Wayland — the backend is a stub and capture does nothing. macOS — does not build.

## Known issues

- Upgrading from HydroShot 1.x: config, history, and the autostart entry are migrated on first run. The old self-installed copy is not removed; run `hydroshot uninstall` once, or let the 2.0 MSI replace the 1.x MSI.
- Linux capture is X11 only and experimental; multi-monitor and HiDPI are not verified there.
- OCR is Windows only. It uses the Windows.Media.Ocr engine, so a language with OCR has to be installed (Settings › Time & language). Small text (around 12 px and under) can come out with mistakes, and a very long, thin strip (roughly fifty times wider than tall) can come out empty.
- Typing with an input method (IME) in a text annotation is new in this version and has had no real use yet.
- The updater is new in this version. Replacing the exe has been run end to end on Windows. Updating a copy installed with the MSI has only been run with a stand-in for the installer, and the Linux side only in tests. Until a release carries `latest.json`, *Check for updates* answers that there is no update information.
- Screen readers get Settings and History on Windows only; the capture overlay itself is not described to them on any platform.
- Imgur upload ships with no client ID. Register one at api.imgur.com and set it in Settings or `SCREEN_MARK_IMGUR_CLIENT_ID`.
- Window capture skips cloaked UWP windows on purpose; some Store apps will not be selectable by click.
- CLI `--delay` waits silently. The on-screen countdown only appears for tray-menu delays.
- CLI `--save` writes PNG, JPEG, WebP or BMP, picked by the file extension; anything else is refused.
- `Ctrl+PrintScreen` and `Shift+PrintScreen` are already taken on some machines (Greenshot, ShareX, and some keyboard utilities register them; Greenshot uses `Shift+PrintScreen` for its own last region). screen-mark shows a popup when a hotkey can't be registered; click it to pick another in Settings › Keys.
- Linux: `screen-mark capture --clipboard` loses the image when the command exits, because X11 clipboards live in the owning process. The tray app's clipboard is fine.

## Offline and air-gapped use

Everything on the release page works with no internet access. Only Imgur upload and looking for updates need a connection, and neither happens unless you ask for it.

- **Windows:** the exe needs Windows 10 or later and nothing else, not even the Visual C++ Redistributable. The `.zip` and the MSI also carry this README, the changelog, the licence and `THIRD-PARTY-LICENSES.txt`.
- **Linux:** the binary uses the system's GTK 3, libxdo, the X11 part of libxkbcommon (for the keyboard) and an AppIndicator library for the tray. Without libxkbcommon-x11 it stops at start with "Library libxkbcommon-x11.so could not be loaded". On Ubuntu, install them with `sudo apt install libxdo3 libxkbcommon-x11-0 libayatana-appindicator3-1` (GTK 3 comes with the desktop) before going offline.
- **Website:** `screen-mark-vX.Y.Z-website.zip` is the whole site, with that release's downloads inside `docs/downloads/`, for an internal web server. Its `.gitlab-ci.yml` publishes it with GitLab Pages.
- **Updates:** `docs/downloads/` in the website zip also holds `latest.json` and its signature, so the same internal server can serve updates. Point screen-mark at it with `url` under `[updates]` (see [Updating](#updating)).
- **Checksums:** `SHA256SUMS.txt` lists every file on the release. Check with `sha256sum -c SHA256SUMS.txt --ignore-missing` on Linux, or compare `Get-FileHash <file>` in PowerShell.

### Building with no internet

`screen-mark-vX.Y.Z-source-vendored.tar.gz` is the source plus every crate it depends on, in `vendor/`. Its `.cargo/config.toml` points cargo there, so no download is needed. Each release checks that it builds with networking switched off. Bring these onto the machine first:

- **Rust.** `BUILT-WITH.txt` in the bundle names the version the release was built with. The [standalone installers](https://forge.rust-lang.org/infra/other-installation-methods.html#standalone-installers) work offline.
- **Windows:** Visual Studio Build Tools with the *Desktop development with C++* workload. `vs_buildtools.exe --layout <folder>` makes an offline installer.
- **Linux:** `libgtk-3-dev libxdo-dev libxkbcommon-dev libwayland-dev libglib2.0-dev` (Ubuntu package names).
- **The MSI only:** `cargo-wix` and the WiX Toolset 3. Install `cargo-wix` while online (`cargo install cargo-wix`). The exe itself needs neither.

```sh
tar -xzf screen-mark-vX.Y.Z-source-vendored.tar.gz
cd screen-mark-vX.Y.Z
cargo build --release --offline
cargo test --offline
```

## Configuration

Written on first run to `%APPDATA%\screen-mark\config.toml` (Windows) or `~/.config/screen-mark/config.toml` (Linux). Every key is optional; missing keys take the defaults below. Set `SCREEN_MARK_HOME` to keep the config and history in one folder of your choosing instead (portable use; the test suite uses it too).

```toml
[general]
default_color = "blue"        # red | blue | green | yellow | mauve | "#rrggbb"
default_thickness = 3.0
save_directory = ""           # where captures go; empty means Pictures/screen-mark
ask_where_to_save = true      # Ctrl+S opens a PNG/JPEG/WebP dialog; false saves straight to the folder
                              # (left unset, it asks only while save_directory is empty)
copy_also_saves = false       # every copy also writes a PNG to save_directory
finish_on_release = false     # copy as soon as a region is drawn, no markup toolbar
magnifier = true              # magnified pixels beside the pointer while placing a region
screenshot_area = "monitor"   # monitor (under the mouse) | all
notifications = "toast"       # toast | system | off; errors always show
imgur_client_id = ""          # or SCREEN_MARK_IMGUR_CLIENT_ID in the environment
history_enabled = true
theme = "dark"                # dark | light

[hotkey]                      # global; "" leaves one unbound
region = "Ctrl+Shift+S"       # was `capture` in 2.0; that name still works
screenshot = "Ctrl+PrintScreen"
last_region = "Shift+PrintScreen"  # the last region's area again, saved and copied

[keys]                        # while marking up
copy = "Ctrl+C"
save = "Ctrl+S"
undo = "Ctrl+Z"
redo = "Ctrl+Shift+Z"
select_all = "Ctrl+A"
pin = ""
ocr = ""
upload = ""

[shortcuts]                   # one key per tool; defaults are the table above
select = "v"

[toolbar]                     # hide tools you never use
measurement = false

[updates]
auto_check = false            # look for a new release after starting and once a day; never installs by itself
url = ""                      # where releases are, when not GitHub: a web folder or a folder path
proxy = ""                    # empty: HTTPS_PROXY / NO_PROXY, then Windows' proxy setting; or "http://host:port", or "none"
```

The log is `%LocalAppData%\screen-mark\screen-mark.log` (Windows), `~/.local/share/screen-mark/screen-mark.log` (Linux), or `screen-mark.log` in `SCREEN_MARK_HOME`. When a capture can't start, the popup says why and a click on it shows the file. It holds messages and file paths, never image data, starts afresh at 1 MB, and keeps the one before as `.old`. `RUST_LOG=debug` makes it say more.

Source layout, for the curious: `src/capture/` has one backend per platform, `src/tools/` has one file per annotation tool, `src/overlay/` is the selection UI and what it does with the mouse and keyboard (`input.rs`, which `main.rs` only feeds events to), `src/state.rs` holds the annotation command stack that undo and redo replay, and `src/update/` is the updater. `docs/` is the [website](https://real-fruit-snacks.github.io/screen-mark/), deployed by `pages.yml` on every push to `main`; `.gitlab-ci.yml` does the same on GitLab for air-gapped hosting.

## License

MIT
