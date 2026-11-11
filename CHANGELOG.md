# Changelog

All notable changes to screen-mark will be documented in this file.

## [Unreleased]

Images that are already there can be marked up, History can bring a capture back with its annotations still editable, shapes can be dashed or filled, and capture works on Wayland.

### Added
- **Mark up an image you already have.** *Mark up an image...* and *Mark up the clipboard image* in the tray menu, and `screen-mark edit [FILE]` from a terminal (no file: the clipboard's image), open it in the same editor: the image in the middle of the screen, selected, with every tool and action. Shrinking the selection crops. Save suggests `<name> (marked up).png`. An image bigger than the screen is scaled to fit, and is copied and saved at that size.
- **History does more than copy.** Buttons along the bottom act on the capture clicked last: *Copy*, *Mark up*, *Save as...*, *Show in folder* and *Delete* (one capture; *Clear All* is still there). From the keyboard: `E`, `S`, `O` and `Delete` on the focused capture, and `Tab` reaches the buttons.
- **Annotations stay editable in History.** A capture with annotations keeps the clean image and the annotations beside its PNG (`<time>.markup`), so *Mark up* brings back the arrows, boxes and text to move, restyle or fix instead of a flat picture. Captures without annotations, and those from earlier versions, open as pictures. Deleting or pruning a capture removes its markup.
- **Dashed and filled shapes.** Rectangles, circles, rounded rectangles, lines and arrows are drawn as an outline, dashed, or filled (lines and arrows: outline or dashed). `D` or the style button beside the colours switches; with a shape selected it restyles that shape, as one undo step. A filled shape can be picked up anywhere on it. `[general] shape_style` sets where each capture starts; `[keys] style` rebinds `D`.
- **Send: hand a capture to your own command.** `[send] command` (with `{file}` for the capture's PNG, or the path last) adds a Send button to the toolbar and a bindable key (`[keys] send`). What the command prints is copied, so an upload script that prints a link leaves the link on the clipboard. No shell runs it; a failure shows the exit code and the last line it printed; it is stopped after two minutes. `[send] name` renames the button.
- **File name patterns.** `[general] file_name` sets what captures are saved as: `{date}`, `{time}`, `{year}` ... `{second}`, `{width}`, `{height}`. The default is the name screen-mark always used. Characters a file name can't hold become `-`. Settings › Capture shows the next name.
- **Wayland capture.** On a Wayland session screen-mark takes the picture through the desktop's screenshot portal (xdg-desktop-portal with the GNOME, KDE or wlr backend) and runs its windows through XWayland. Wayland has no global hotkeys: bind a desktop shortcut to `screen-mark capture`. `SCREEN_MARK_CAPTURE=x11` or `portal` overrides the choice. No new packages: `ashpd`, `pollster` and `url` were already in the tree through the file dialogs.
- **The capture overlay talks to screen readers (Windows).** Its toolbar's buttons, which tool, colour and shape style are on, and a status line: the selection's size, the tool, how many annotations, and the keys that finish or cancel. A screen reader can press any toolbar button.

### Changed
- `screen-mark capture --delay N` counts down on screen, like the tray menu's delays, and prints that it is waiting. With `--clipboard` or `--save` it used to wait without a word, and the countdown could not have been in the picture: it is gone before the capture.
- Settings › General says whether Imgur upload and Send are set up, in one line; Settings › Capture shows the file name pattern's next name.
- The toolbar has two more buttons (Style always, Send when a command is set).

### Fixed
- **Linux: links from Imgur upload and text from OCR left the clipboard at once.** They were copied through a clipboard handle that was dropped straight away, and on X11 the clipboard lives only as long as its owner. They now go through the same long-lived handle as images.

## [2.3.0] - 2026-11-11

screen-mark can now update itself, there is a Redact tool, Settings and History work from the keyboard and with a screen reader, and the overlay redraws only what changed. 2.2.1 has no updater, so install this release by hand; later ones can be installed from the tray menu or with `screen-mark update`.

### Added
- **Redact tool** (`X`): a solid box that keeps nothing of what is under it. Pixelate keeps a block average, which is not always enough to hide text. A Pixelate drawn across a Redact box averages the box, never what it hides, whichever was drawn first. Black by default; select it and click a swatch to recolour. Fifteen tools now; `[shortcuts] redact` and `[toolbar] redact` in the config.
- **Text annotations have a caret and can have several lines.** `Shift+Enter` starts a new line (`Enter` still finishes), the arrow keys, `Home`, `End` and `Delete` work, a click on the text moves the caret, and pasted text keeps its lines. Input methods (IME) are passed through while typing; this part has had no real use yet.
- **Characters the built-in font lacks** (Chinese, Japanese, Korean, emoji, ...) are drawn with a font installed on the system, in text annotations and in popups. Nothing is read until one turns up.
- **Pixel-exact selection.** A magnifier beside the pointer while a region is placed, dragged out or resized (`[general] magnifier`, and a switch in Settings › Capture). Arrow keys move the pointer one pixel (ten with `Shift`); with a region they move it, `Ctrl`+arrows resize it; with an annotation selected they move that, as one undo step. `Shift` while dragging gives a square region, squares and circles, and lines at 15° steps.
- **Settings and History work from the keyboard.** `Tab`, `Shift+Tab` and the arrow keys move between controls with a visible focus ring, `Enter` or `Space` presses, `Ctrl+Tab` changes tab, `Ctrl+Enter` saves, `Esc` closes. In History the arrows move over the grid.
- **Screen readers (Windows).** Settings and History are described to UI Automation through AccessKit: every control's role, name, state and place, and the message line. `cargo run --example access_probe` opens Settings alone for checking with Narrator or Accessibility Insights.
- **A log file**, `screen-mark.log` next to the history (`%LocalAppData%\screen-mark` on Windows). The tray app has no console, so its log used to go nowhere. It starts afresh at 1 MB. A panic is written to it with a backtrace, and the tray app shows a dialog naming the file instead of vanishing.
- **Updating from inside screen-mark.** *Check for updates* in the tray menu looks for a newer release; if there is one the item becomes *Install screen-mark X.Y.Z*, which downloads it, puts it in place and restarts. `screen-mark update` does the same from a terminal, and `screen-mark update --check` only looks. Nothing is fetched unless you ask: *Check for a new version once a day* in Settings › General (`[updates] auto_check`) is off by default, and when it is on it tells you and installs nothing by itself.
  - A release is installed only if its `latest.json` is signed (Ed25519) with one of two keys compiled into screen-mark, each download has the size and SHA-256 that file gives, and the new program starts and reports the release's version. A release that isn't newer than the running one is never installed. The version that was replaced stays beside the new one until the new one has started.
  - A self-installed or unpacked copy has its program file replaced. A copy from the MSI runs the new MSI. A copy from Scoop, winget or Chocolatey, or in a folder screen-mark can't write to, is left alone.
  - `[updates] url` points it at a web folder or a folder path instead of GitHub; the website zip's `docs/downloads/` is one, so an internal server can serve updates. A folder can also be picked in Settings › General.
  - Behind a proxy it uses `HTTPS_PROXY` / `HTTP_PROXY` with `NO_PROXY`, then on Windows the proxy in Windows' settings with its exceptions. `[updates] proxy` names one (or `none`) for a proxy set by a PAC script.
  - No new packages: the signature and checksum code is `ring` and the unpacking is `flate2`, both already in the tree.
- Releases carry `latest.json` and `latest.json.sig`, written and signed by the release workflow (`.github/scripts/update-manifest.sh`). A tagged release fails before publishing if the signing key is missing or is not one screen-mark trusts.

### Changed
- **The overlay redraws only what changed.** Every pointer move used to copy the whole desktop, draw on it, convert all of it and hand all of it to the window. Now a frame puts the background back where the last one drew, draws again, and gives the window just those areas. Timed with `cargo run --release --example overlay_bench` (screen-mark's own work per frame; the system's copy to the screen shrinks the same way and is not in these numbers): on one 4K screen 9–11 ms became 0.3–1.7 ms, on two 17–18 ms became 0.4–1.6 ms. After a click or a key the frame is still drawn whole, about 8 ms and 18 ms. `tests/overlay_frame_tests.rs` holds the patched frame to a freshly drawn one through random sessions.
- **The overlay keeps three copies of the desktop instead of five**: about 100 MB on a 4K screen where it was 166, 199 MB on two where it was 332. The dimmed copy is gone (the background is dimmed as it is painted), and so is a second copy of the capture. Opening the overlay does less for the same reason: about 19 ms of work after the capture instead of 31 on one 4K screen, 25–30 instead of 61 on two.
- A spotlight dims the picture only. It used to dim the selection border, the crosshair and the size label with it.
- **OCR runs in-process** on the Windows.Media.Ocr engine instead of through a PowerShell script with `-ExecutionPolicy Bypass`. It takes a few milliseconds instead of the quarter of a second and more that starting PowerShell costs, writes no temp copy of the capture to disk, and works where PowerShell is locked down. Lines of text are copied as lines. A capture wider than the engine's 10 000 px limit is scaled to fit.
- A region capture that can't start (the screen can't be read, the overlay can't open) now says so in a popup; a click on it shows the log. It used to do nothing visible. Settings, History and auto-start failures are reported the same way.
- A pin's middle-click copies the capture itself, without the pin's border and shadow, and the temp file a right-click reveals is the capture too. The copy is reported in the usual popup, so `notifications = "off"` applies to it. Copying from History is reported the same way.
- `screen-mark capture --save` writes JPEG correctly (it used to fail on the alpha channel) and accepts PNG, JPEG, WebP or BMP; other extensions are refused with a message that says so.
- The minimum Rust version is 1.88, which is what the locked dependencies need. `rust-version` said 1.80, but the locked `image` 0.25.10 alone needs 1.88. CI now builds with the declared version.
- `image` is built with PNG, JPEG, WebP and BMP only. The default feature set also compiled an AVIF encoder, OpenEXR and TIFF. With the Windows accessibility crates added, the lockfile goes from 517 packages to 487.
- The overlay's mouse and keyboard handling moved out of `main.rs` into `src/overlay/input.rs`, where tests drive it without a window. The overlay and the export draw annotations with the same code, and a test compares the region on screen with the copied image pixel for pixel.
- Window capture reads the list of windows when the capture is taken, not when the tray menu was clicked.

### Fixed
- **Spotlight was missing from the copied or saved image.** It showed in the overlay only.
- **Linux: the install instructions missed `libxkbcommon-x11-0`.** Without it screen-mark stops as it starts ("Library libxkbcommon-x11.so could not be loaded"). The README and the website list it now. The same gap had kept the end-to-end tests from ever starting the app on CI since 2.1.0; a failed test now prints what the app logged.
- Clicking somewhere else while typing a text annotation threw the text away; it now finishes the text. Copy, Save, Pin and Upload from the toolbar include text that is still being typed.
- The text caret was white and invisible on a white page; it takes the text's colour.
- A drag of a pixel or two left a selection too small to see, with the toolbar attached.
- Picking *Capture window* while a delayed capture was counting down left that capture, or the next one, in window mode.

## [2.2.1] - 2026-11-05

Everything needed to run and build screen-mark with no internet access is now in the release. See *Offline and air-gapped use* in the README.

### Changed
- **The Windows exe runs on a PC without the Visual C++ Redistributable.** The C runtime is now linked into `screen-mark.exe`, so it no longer needs `VCRUNTIME140.dll`. Before, a clean PC without that runtime couldn't start it.
- The Windows `.zip`, the MSI and the Linux `.tar.gz` now include the README, the changelog, the licence and `THIRD-PARTY-LICENSES.txt`, the licences of the crates built into the binary.
- The website zip now carries the release's downloads in `docs/downloads/`, and its download links point at them, so a copy on an offline web server works.
- Website: the demo overlay works from the keyboard and with a screen reader, the first screen fits laptop and phone screens, and the install steps are clearer.

### Added
- `screen-mark-vX.Y.Z-source-vendored.tar.gz`: the source plus every crate it depends on. It builds with `cargo build --offline`, and each release checks that with networking switched off.
- `SHA256SUMS.txt`, with a checksum for every file on the release.

## [2.2.0] - 2026-11-05

### Added
- **Capture the last region again.** `Shift+PrintScreen` captures the same area of the screen as the last region capture, with no overlay, then saves a PNG and copies it like the one-press screenshot. The area is remembered when a region is copied, saved, pinned, uploaded or read with OCR, and kept across restarts in `last-region.txt` next to the history. With no region yet, or if that area is on no screen any more (a monitor was unplugged), a popup says so. Rebindable in Settings › Keys as `[hotkey] last_region`, and in the tray menu as *Last region again (save + copy)*.

## [2.1.0] - 2026-10-29

### Added
- **One-press screenshot.** `Ctrl+PrintScreen` captures the monitor under the mouse, saves a PNG to the captures folder, and copies it to the clipboard. Settings › Capture switches it to all monitors. Presses made while a capture is still finishing are queued, not dropped, and file names never overwrite each other (`_2`, `_3`, ...).
- **Popups after a capture.** A small corner popup on the monitor under the mouse says what was saved and copied, with a thumbnail; click it to open the folder with the file selected. It never takes focus and is kept out of the next capture. Settings › Capture can use system notifications instead, or turn them off; errors always show.
- **Save when copying.** *Save a file every time I copy* writes a PNG next to each copy from the editor.
- **Skip the markup toolbar.** *Copy as soon as a region is drawn* finishes a region capture on mouse release (and a window capture on click).
- **Every key is rebindable** in a new Settings › Keys tab: both global hotkeys, the editor's copy / save / undo / redo / select-all, pin, OCR and upload (unbound by default), and all fourteen tool keys. Click a binding and press the keys; `Esc` cancels, `Backspace` unbinds. Duplicates, combinations the OS keeps for itself, keys that can't work in their place (a bare letter as a global hotkey, `Enter` as a tool key, punctuation as a global hotkey), and global hotkeys another app holds are refused with the reason; a global hotkey that hides a common shortcut from other apps (`Alt+S`, a bare `F5`) is allowed with a note. Letters follow the keyboard layout; digits and punctuation follow the key position. Everything a 2.0 config could hold keeps working, including tool keys set to any typed character.
- If a global hotkey can't be registered at startup, a popup names it; click it to land on that key in Settings.
- Tray menu: *Screenshot (save + copy)* and *Open captures folder*, and both capture items show their current hotkey.
- `SCREEN_MARK_HOME` keeps config and history in one folder (portable use).
- `tests/e2e/`: end-to-end usage tests that drive the real binary on a virtual X display with two monitors, a tray host, and a fake notification server. Run in CI on Ubuntu.

### Changed
- `[hotkey] capture` is now `[hotkey] region`; the old name is still read. New `screenshot`, `[keys]`, and `[general]` keys are listed in the README.
- With no save folder set, the one-press screenshot and save-on-copy use `Pictures/screen-mark`. `Ctrl+S` still asks where to save until you choose a folder, as before; the new *Save asks where to save* switch makes that a choice.
- Save errors say why first ("permission denied", "disk full", "a file is in the way") and then the path.
- Windows: the process is per-monitor DPI aware, and notifications carry the screen-mark name and icon.

### Fixed
- Linux: the tray icon panicked at startup ("GTK has not been initialized"); it now runs on its own GTK thread.
- Linux: global hotkey presses waited for the next window event before anything happened.
- Linux: holding a global hotkey down fired it again on every key repeat.
- Linux: an image copied from the tray app could drop off the clipboard, because the clipboard handle was closed after each copy.

## [2.0.0] - 2026-10-26

### Changed
- **Renamed to `screen-mark`.** Binary, crate, repo, config directory (`screen-mark/`), history directory, env var (`SCREEN_MARK_IMGUR_CLIENT_ID`; the old name still works), autostart entry, Start Menu entry, and MSI product name. On first run, a 1.x config and history are copied across and the 1.x autostart entry is replaced; the 2.0 MSI upgrades a 1.x MSI install. A 1.x portable self-install is left in place — run `hydroshot uninstall` once.
- Retheme to the RFS palettes, **Nightshift** (dark) and **Dayshift** (light), with Ember as the one accent, per [BRAND.md](https://github.com/Real-Fruit-Snacks/.github/blob/main/BRAND.md). Replaces Terminal Workbench across the app, the website, the cover art, the favicon, and the app icon. Annotation presets keep their names; `green` is now Mint `#3ddc97`, `yellow` is Amber `#ffc247`, `blue` is `#5cb8ff`. Existing `default_color` values in `config.toml` keep working.
- Website set in IBM Plex Sans / Plex Mono (vendored, still offline). Site and README copy rewritten to the RFS house voice; repo renamed `hydroshot`.
- App icon is a flat Ember drop in a graphite tile; no gradients.
- `hydroshot --version` now works.

### Added
- `tests/theme_tests.rs` pins the palette to BRAND.md and checks WCAG AA contrast for every accent in both modes.
- `cargo run --example render_theme` renders the capture overlay headlessly in both themes to PNG, for reviewing palette changes without a display.

## [1.1.3] - 2026-07-12

### Changed
- Recolored the application icon to the Terminal Workbench palette — a graphite tile with mint selection brackets framing a cyan water droplet. It now matches the app, the website favicon, and the theme across the window, taskbar, tray, executable, and installer.

## [1.1.2] - 2026-07-12

### Changed
- Attribution is now consistently **Real-Fruit-Snacks** across the LICENSE, the executable's embedded copyright and company name, the MSI installer's manufacturer and bundled license, and the crate metadata.

## [1.1.1] - 2026-07-12

### Changed
- Rebuilt the project website on the Terminal Workbench product-page layout — fixed nav, a hero with a live capture-overlay mock, feature cards, a workflow guide, and install cards — so it matches the design system's other sites. Redrew the cover art in the calm, centered house style. The site remains fully self-hosted and offline (vendored fonts, no CDN). Documentation only; no application changes.

## [1.1.0] - 2026-07-12

### Added
- **Light mode.** The whole app (capture overlay, toolbar, settings, and history windows) now renders in light as well as dark. Switch it from Settings › General, or set `theme = "dark" | "light"` in `config.toml`.

### Changed
- **Retheme to the [Terminal Workbench](https://github.com/Real-Fruit-Snacks/terminal-workbench-design-system) design system.** Graphite surfaces, quiet chrome, and restrained accents replace the previous palette across both the app and the website. The five annotation presets (red, blue, green, yellow, mauve) now map to the theme's semantic hues; existing `default_color` names in `config.toml` keep working.
- The project website is rebuilt on the Terminal Workbench tokens with self-hosted fonts, so it works fully offline; a GitLab Pages workflow is included for air-gapped hosting.

### Security
- Updated `rustls-webpki` 0.103.10 → 0.103.13 (TLS stack used for Imgur uploads): fixes RUSTSEC-2026-0098/0099 (certificate name-constraint bypasses) and RUSTSEC-2026-0104 (reachable panic in CRL parsing)

### Added
- Experimental X11 region capture on Linux (root-window GetImage via x11rb) — the Linux build can actually capture now
- Global hotkey is rebindable from Settings > General (click the binding, press the new combo); digits are now valid hotkey keys
- History can be disabled in Settings, and the History window gained scrolling, a scrollbar, and a "Clear All" button
- Ctrl+V pastes clipboard text while typing a Text annotation; Space works in text input
- `--clipboard` and `--save` CLI flags can be combined
- `default_color` accepts `#rrggbb` hex values
- Dependency-audit job (cargo-audit, advisory) in CI

### Changed
- The portable exe now ASKS before installing itself (Yes installs, No runs portably); debug builds never self-install, so `cargo run` works normally
- Enter now copies the selection with annotations (same path as Ctrl+C) instead of silently dropping them
- Highlighter uses multiply blending — text stays readable and overlaps don't wash out
- History moved from the roaming config dir to local app data (existing entries are migrated)
- Pin temp files are cleaned up on app exit, not just on pin close
- Text annotation re-edit is a single undo step (Modify) and Escape restores the original

### Fixed
- CI: new stable clippy lint (`manual_checked_ops`) broke the build
- Latent panic in `flatten_annotations` when capture alpha != 255; Windows capture now forces opaque alpha
- Caps Lock no longer breaks Ctrl+C/S/Z or tool shortcuts, and no longer turns undo into redo
- Pinned windows and the countdown now position correctly on multi-monitor setups (virtual-desktop offsets)
- History window could only show ~12 of 20 entries (no scrolling) and froze the UI while copying
- Window-capture mode: rects use DWM extended frame bounds (no shadow margins), cloaked UWP windows are skipped, and self-exclusion is by process id instead of window title
- Settings "Browse" no longer freezes the event loop (tray/hotkey stay responsive)
- Imgur upload no longer encodes the PNG on the UI thread; the unreachable "Uploading..." toast was removed
- OCR: PowerShell console no longer flashes; missing OCR language packs produce a clear error
- Autostart registry/desktop entries quote the exe path (paths with spaces)
- Config files missing individual keys no longer reset the whole config; parse errors show a notification
- Step-marker numbering stays consecutive after undo/redo/delete
- Pixelate/spotlight regions no longer shift when the rect extends past the selection edge
- Text annotation hit-testing and selection handles use real font metrics instead of estimates
- Toast no longer forces continuous re-renders; hotkey events are fully drained each cycle

## [0.5.9] - 2026-03-28

### Fixed
- Window capture mode now filters all HydroShot windows (Settings, History, Pin) not just the overlay
- Installer path comparison no longer fails due to Windows `\\?\` UNC prefix mismatch
- Settings window now scales correctly on high-DPI displays (LogicalSize + nearest-neighbor scaling)
- Single-instance enforcement prevents duplicate tray icons when launched twice

## [0.5.8] - 2026-03-28

### Fixed
- Self-installer no longer kills itself during install (excluded current PID from taskkill)
- Color picker dialog no longer freezes the app (overlay hides while dialog is open)
- SendMessageTimeoutW replaced with non-blocking PostMessageW in PATH registration
- WiX MSI installer switched to WixUI_Minimal to prevent UI deadlock on per-user install
- PowerShell command injection in shortcut creation (now uses -EncodedCommand)
- Registry handle leak on error paths in add_to_path/remove_from_path
- OCR and pin clipboard operations moved to background threads (no more UI freezing)
- Event loop panics now show user-friendly error dialog instead of silent crash
- Right-click no longer closes overlay when a selection exists
- Autostart checkbox in tray menu now updates immediately after toggling
- build.rs winres compile error is now fatal instead of silently swallowed
- Fixed README: corrected config key (rounded_rect), redo shortcut (Ctrl+Shift+Z), --save usage
- Fixed docs/index.html version badge (v0.5.4 → v0.5.8)
- Added cargo:rerun-if-changed for icon.png in build.rs
- Removed hardcoded version from AppUserModelID

## [0.5.7] - 2026-03-27

### Fixed
- Self-installer autostart now registers the installed exe path, not the source/Downloads path
- Upgrades no longer fail when HydroShot is already running (kills existing process, rename-replace fallback)
- No more console window when running as tray app (windows_subsystem = "windows")
- Install/uninstall now shows message box feedback instead of invisible println! output
- CLI subcommands (capture, install, uninstall) attach to parent console for terminal output
- MSI uninstall now cleans up autostart registry entry and HydroShot registry key

## [0.5.6] - 2026-03-27

### Changed
- EXE now auto-installs when double-clicked from outside the install location

## [0.5.5] - 2026-03-27

### Added
- Self-installer: `hydroshot install` copies to %LocalAppData%\HydroShot, creates Start Menu shortcut, adds to PATH, and enables autostart
- `hydroshot uninstall` command to cleanly remove the installation

### Fixed
- MSI installer no longer requires admin rights — installs to LocalAppData instead of Program Files

## [0.5.4] - 2026-03-25

### Fixed
- Upload button now shows a helpful message immediately when Imgur is not configured, instead of going through the confirmation flow first
- Fixed README config example using hex color code (`#89b4fa`) that silently fell back to red — now uses named color `blue`
- Imgur client ID is now a config.toml setting (`imgur_client_id` under `[general]`) — no environment variable needed for normal use
- Settings UI shows Imgur upload configuration status
- Fixed flaky history tests on Linux CI by serializing with a mutex

## [0.5.3] - 2026-03-25

### Security
- Removed hardcoded Imgur client ID from source code — now configured via Settings/config.toml (env var `HYDROSHOT_IMGUR_CLIENT_ID` as override)
- Replaced hand-rolled JSON parser with `serde_json` to eliminate panic on malformed Imgur responses
- Fixed OCR temp file race condition with unique filenames and Drop-guard cleanup
- Added `-ExecutionPolicy Bypass` to PowerShell OCR invocation for restricted systems
- Pinned all GitHub Actions to SHA hashes to prevent supply chain attacks
- Added `permissions: contents: read` to CI workflow (principle of least privilege)

### Fixed
- Fixed negative selection coordinates wrapping to huge values when cast to u32 — added `Selection::clamped()` helper
- Fixed silent undo/redo action loss when indices become stale — actions are now validated before popping
- Fixed text cursor positioning using hardcoded 0.6 char width — now measures actual font advance widths
- Fixed 1px glyph jitter from float-to-int truncation — now uses rounding
- Fixed color precision loss in float-to-u8 conversion — now rounds instead of truncating
- Fixed settings UI triggering redraws on every sub-pixel mouse move — now only redraws when hovered element changes
- Fixed undo stack `remove(0)` O(n) performance — replaced with `drain()`
- Made `Config::save()` atomic via temp file + rename to prevent corruption on crash
- Fixed `StepMarkerTool` u32 overflow panic at `MAX` — uses `saturating_add`
- Fixed `Color::new()` accepting out-of-range values — now clamps to `[0.0, 1.0]`
- Fixed `Color::presets()` allocating a new Vec every frame — now returns `&'static [Color]`
- Fixed `IconCache::get_or_render` double HashMap lookup — uses entry API
- Fixed state.rs silently skipping pixel copy on truncated buffers — now logs warnings
- Fixed state.rs potential integer overflow in pixel buffer sizing on 32-bit — uses `checked_mul`
- Added minimum size checks to 7 annotation tools to prevent invisible zero-size annotations on click
- Fixed hotkey `letter_to_code` silently mapping unknown characters to KeyA — now returns an error
- Added warning when registering global hotkey without modifier keys
- Added F7-F12 support to hotkey parser
- Added 30-second HTTP timeout to Imgur upload requests

### Changed
- MSRV aligned to 1.80 across Cargo.toml, CONTRIBUTING.md, and CHANGELOG
- Added clippy and format checks to Linux CI job
- Generated proper UUID for WiX installer UpgradeCode (was a placeholder)
- Fixed `cd hydroshot` to `cd HydroShot` in webpage build instructions (case-sensitive Linux)
- Fixed footer copyright to match LICENSE
- Fixed querySelector("#") console error on brand logo click
- Updated Cargo.toml description to be platform-neutral

## [0.5.2] - 2026-03-25

### Fixed
- Fixed command injection vulnerability in OCR PowerShell integration
- Imgur client ID now required via `HYDROSHOT_IMGUR_CLIENT_ID` environment variable (removed hardcoded default)
- Fixed integer overflow in screen capture buffer sizing on multi-monitor setups
- Replaced panicking unwrap/expect calls with graceful error handling
- Moved Imgur upload to background thread to prevent UI freezing
- Fixed string slicing panics on non-ASCII text (CJK, emoji)
- Fixed selection resize allowing negative dimensions
- Fixed alpha blending precision loss in text rendering
- Added bounds checking in crop_and_flatten to prevent panics
- Fixed toolbar positioning going off-screen
- Fixed hardcoded DPI scale factor (now queries actual system DPI)
- Fixed version mismatches across CHANGELOG, webpage, and installer
- Fixed README: correct tool count (14, not 16), accurate project structure, matching config example, correct Linux build deps
- Fixed webpage: correct tool count, removed non-tool from tools grid, clarified Linux Wayland support
- Fixed installer help URL pointing to wrong GitHub repository
- Added `rust-version` to Cargo.toml to enforce MSRV 1.80

## [0.5.0] - 2026-03-24

### Added

#### New Annotation Tools
- Rounded Rectangle tool (O key) — rectangle with adjustable corner radius
- Spotlight tool (F key) — draw rectangles that dim everything outside them
- Measurement tool (M key) — click two points to show pixel distance
- Color Eyedropper (I key) — pick any color from the screenshot

#### Annotation Improvements
- Annotation resize — drag corner handles on selected annotations to resize
- Command-pattern undo/redo — covers all operations (add, delete, move, resize, recolor)

#### Export & Sharing
- Imgur upload — toolbar Upload button with confirmation click
- OCR text extraction — extract text from selected region using Windows OCR
- Recent captures history — tray History menu shows thumbnails, click to re-copy

#### Configuration
- Customizable keyboard shortcuts — rebind all tool shortcuts in Settings UI
- Configurable toolbar — hide/show individual tools in Settings Toolbar tab
- Tabbed Settings UI — General, Shortcuts, and Toolbar tabs

#### Interface & UX
- In-overlay toast notifications — feedback shown directly on the overlay
- Pin window improvements — right-click to reveal in Explorer, middle-click to copy, draggable

#### Build & Distribution
- Windows MSI installer via CI
- GitHub Actions CI — automated builds, tests, and releases
- Embedded exe icon — HydroShot icon in Windows Explorer and taskbar

#### Performance
- Cached font rendering
- Cached pixmaps for annotation tools
- 60fps frame rate cap
- Professional Lucide SVG icons via resvg

## [0.2.0] - [0.4.0] — Internal development releases

## [0.1.0] - 2026-03-25

### Added
- System tray application with left-click capture
- Global hotkey (Ctrl+Shift+S) for instant capture
- Fullscreen overlay with region selection (drag, resize, move)
- 10 annotation tools: Select/Move, Arrow, Rectangle, Circle, Line, Pencil, Highlight, Text, Pixelate, Step Markers
- Catppuccin Mocha color presets with native color picker (right-click swatch)
- Quick crop mode (Enter key)
- Copy to clipboard (Ctrl+C) and save to file (Ctrl+S)
- Pin captures to screen as always-on-top floating windows
- Window capture mode (highlight and click a window)
- Delay capture (3s/5s/10s) with visible countdown overlay
- Multi-monitor support (captures entire virtual desktop)
- CLI interface (`hydroshot capture --clipboard/--save/--delay`)
- In-app Settings UI
- TOML configuration persistence
- Auto-start on login
- Cursor feedback, selection size overlay, tooltips
- Post-action notifications
- Undo/redo for annotations
- Annotation re-selection (move, delete, recolor existing annotations)
- Keyboard shortcuts for all tools
- Performance optimized rendering (cached pixmaps, 60fps cap)
