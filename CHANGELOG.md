# Changelog

All notable changes to screen-mark will be documented in this file.

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
