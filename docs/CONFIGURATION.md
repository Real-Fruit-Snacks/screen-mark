# screen-mark Configuration & Architecture

Written on first run to `%APPDATA%\screen-mark\config.toml` (Windows) or `~/.config/screen-mark/config.toml` (Linux). Every key is optional; missing keys take the defaults below. Set `SCREEN_MARK_HOME` to keep the config and history in one folder of your choosing instead (portable use; the test suite uses it too).

## `config.toml`

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
shape_style = "outline"       # outline | dashed | filled; D switches while marking up
file_name = "screen-mark_{date}_{time}"  # see USAGE.md#file-names

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
send = ""                     # runs [send] command
style = "d"                   # outline / dashed / filled; one key, like a tool

[shortcuts]                   # one key per tool; defaults are in USAGE.md#annotation-tools
select = "v"

[toolbar]                     # hide tools you never use
measurement = false

[updates]
auto_check = false            # look for a new release after starting and once a day; never installs by itself
url = ""                      # where releases are, when not GitHub: a web folder or a folder path
proxy = ""                    # empty: HTTPS_PROXY / NO_PROXY, then Windows' proxy setting; or "http://host:port", or "none"

[send]
command = ""                  # empty: no Send button. See USAGE.md#send
name = ""
```

## Logs

The log is `%LocalAppData%\screen-mark\screen-mark.log` (Windows), `~/.local/share/screen-mark/screen-mark.log` (Linux), or `screen-mark.log` in `SCREEN_MARK_HOME`. When a capture can't start, the popup says why and a click on it shows the file. It holds messages and file paths, never image data, starts afresh at 1 MB, and keeps the one before as `.old`. `RUST_LOG=debug` makes it say more.

## Source Layout

- `src/capture/` — one capture backend per platform (Windows, X11, Wayland portal).
- `src/tools/` — one file per annotation tool.
- `src/overlay/` — selection UI, rendering, and keyboard/mouse input (`input.rs`).
- `src/state.rs` — annotation command stack that undo and redo replay.
- `src/update/` — Ed25519-signed release updater.
- `docs/` — the [project website](https://real-fruit-snacks.github.io/screen-mark/), deployed by `pages.yml` on every push to `main` (`.gitlab-ci.yml` mirrors it for air-gapped hosting).
