#!/usr/bin/env python3
"""End-to-end usage tests for screen-mark on X11.

Runs the real binary on a virtual display (Xvfb + openbox + stalonetray),
presses its global hotkeys with xdotool, drags regions with the mouse,
clicks its popup and its tray menu, rebinds keys in the real Settings
window, and checks what a user would check: the file in the folder, the
image on the clipboard, the popup on screen, config.toml.

Usage: tests/e2e/run.sh [path/to/screen-mark]   (sets up the display)
       python3 tests/e2e/e2e.py <binary> [test-name-substring]
"""

import hashlib
import io
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import traceback

from PIL import Image

BIN = os.path.abspath(sys.argv[1])
ONLY = sys.argv[2] if len(sys.argv) > 2 else ""
W, H = 1920, 1080
LEFT_RGB = (0x20, 0x50, 0xA0)
RIGHT_RGB = (0xA0, 0x40, 0x20)
POPUP_NAME = "screen-mark notification"
ARTIFACTS = os.environ.get("E2E_ARTIFACTS", os.path.join(tempfile.gettempdir(), "sm-e2e-artifacts"))


# ───────────── small helpers ─────────────

def sh(*args, check=True, capture=True, env=None, timeout=20):
    r = subprocess.run(args, capture_output=capture, env=env, timeout=timeout)
    if check and r.returncode != 0:
        raise RuntimeError(f"{args} failed: {r.stderr.decode(errors='replace')}")
    return r.stdout.decode(errors="replace") if capture else ""


def wait_for(pred, timeout=5.0, step=0.05, what="condition"):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        last = pred()
        if last:
            return last
        time.sleep(step)
    raise AssertionError(f"timed out after {timeout}s waiting for {what}")


def never(pred, duration=1.5, step=0.1, what="condition"):
    end = time.time() + duration
    while time.time() < end:
        if pred():
            raise AssertionError(f"did not expect {what}")
        time.sleep(step)


def windows(name):
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", f"^{name}$"],
                       capture_output=True, text=True)
    return [w for w in r.stdout.split() if w]


def overlays():
    """The capture overlay (the tray icon is also called screen-mark)."""
    return [w for w in windows("screen-mark") if geometry(w)[2] > 400]


def geometry(win):
    out = sh("xdotool", "getwindowgeometry", "--shell", win)
    d = dict(line.split("=", 1) for line in out.split() if "=" in line)
    return int(d["X"]), int(d["Y"]), int(d["WIDTH"]), int(d["HEIGHT"])


def key(*combos):
    for c in combos:
        sh("xdotool", "key", "--clearmodifiers", c)


def move(x, y):
    sh("xdotool", "mousemove", str(x), str(y))


def drag(x0, y0, x1, y1):
    sh("xdotool", "mousemove", str(x0), str(y0), "sleep", "0.1", "mousedown", "1",
       "sleep", "0.1", "mousemove", str((x0 + x1) // 2), str((y0 + y1) // 2), "sleep", "0.05",
       "mousemove", str(x1), str(y1), "sleep", "0.1", "mouseup", "1")


def client_origin(win):
    """Top-left of the window's content (inside any frame the WM added)."""
    out = sh("xwininfo", "-id", win)
    vals = {}
    for line in out.splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            vals[k.strip()] = v.strip()
    return int(vals["Absolute upper-left X"]), int(vals["Absolute upper-left Y"])


def click_in(win, x, y, button=1):
    wx, wy = client_origin(win)
    sh("xdotool", "mousemove", str(wx + x), str(wy + y), "sleep", "0.1", "click", str(button))


def root_png():
    data = subprocess.run(["import", "-window", "root", "png:-"], capture_output=True).stdout
    return Image.open(io.BytesIO(data)).convert("RGB")


def clipboard_png():
    r = subprocess.run(["xclip", "-selection", "clipboard", "-t", "image/png", "-o"],
                       capture_output=True, timeout=5)
    if r.returncode != 0 or not r.stdout:
        return None
    try:
        return Image.open(io.BytesIO(r.stdout)).convert("RGB")
    except Exception:  # noqa: BLE001 - not (yet) an image
        return None


def clipboard_hash():
    r = subprocess.run(["xclip", "-selection", "clipboard", "-t", "image/png", "-o"],
                       capture_output=True, timeout=5)
    return hashlib.sha256(r.stdout).hexdigest() if r.returncode == 0 else None


def clear_clipboard():
    subprocess.run(["xclip", "-selection", "clipboard", "-i"], input=b"cleared", timeout=5)


def close(rgb, want, tol=6):
    return all(abs(a - b) <= tol for a, b in zip(rgb, want))


def save_artifact(img, name):
    os.makedirs(ARTIFACTS, exist_ok=True)
    img.save(os.path.join(ARTIFACTS, name))


# ───────────── the scene ─────────────

def paint_background():
    """Two colours, so a crop shows which monitor was captured."""
    bg = Image.new("RGB", (W, H), LEFT_RGB)
    bg.paste(Image.new("RGB", (W // 2, H), RIGHT_RGB), (W // 2, 0))
    path = os.path.join(tempfile.gettempdir(), "sm-e2e-bg.png")
    bg.save(path)
    sh("display", "-window", "root", path, check=False)  # paints, then exits 1


def two_monitors(on):
    subprocess.run(["xrandr", "--delmonitor", "left"], capture_output=True)
    subprocess.run(["xrandr", "--delmonitor", "right"], capture_output=True)
    if on:
        sh("xrandr", "--setmonitor", "left", f"{W//2}/254x{H}/286+0+0", "screen")
        sh("xrandr", "--setmonitor", "right", f"{W//2}/254x{H}/286+{W//2}+0", "none")


# ───────────── the app ─────────────

class App:
    def __init__(self, config_toml="", extra_env=None, args=()):
        self.root = tempfile.mkdtemp(prefix="sm-e2e-")
        self.home = os.path.join(self.root, "home")
        self.shots = os.path.join(self.root, "shots")
        for d in ("home", "config/screen-mark", "data", "bin"):
            os.makedirs(os.path.join(self.root, d), exist_ok=True)
        self.config_path = os.path.join(self.root, "config/screen-mark/config.toml")
        toml = config_toml.replace("{shots}", self.shots).replace("{root}", self.root)
        if "save_directory" not in toml:
            toml = toml.replace("[general]", f'[general]\nsave_directory = "{self.shots}"', 1) \
                if "[general]" in toml else f'[general]\nsave_directory = "{self.shots}"\n' + toml
        with open(self.config_path, "w") as f:
            f.write(textwrap.dedent(toml))
        # A fake xdg-open records what the popup asked to open.
        self.opened = os.path.join(self.root, "opened.txt")
        fake = os.path.join(self.root, "bin", "xdg-open")
        with open(fake, "w") as f:
            f.write(f'#!/bin/sh\necho "$@" >> "{self.opened}"\n')
        os.chmod(fake, 0o755)
        env = dict(os.environ)
        env.update({
            "HOME": self.home,
            "XDG_CONFIG_HOME": os.path.join(self.root, "config"),
            "XDG_DATA_HOME": os.path.join(self.root, "data"),
            "PATH": os.path.join(self.root, "bin") + ":" + env.get("PATH", ""),
            "RUST_LOG": "info",
            "NO_AT_BRIDGE": "1",
        })
        env.update(extra_env or {})
        self.log_path = os.path.join(self.root, "app.log")
        self.logf = open(self.log_path, "w")
        self.proc = subprocess.Popen([BIN, *args], env=env, stdout=self.logf, stderr=subprocess.STDOUT)

    def log(self):
        with open(self.log_path, errors="replace") as f:
            return f.read()

    def wait_log(self, text, timeout=8):
        return wait_for(lambda: text in self.log(), timeout, what=f"log line {text!r}")

    def files(self):
        if not os.path.isdir(self.shots):
            return []
        return sorted(f for f in os.listdir(self.shots) if f.endswith(".png"))

    def config(self):
        with open(self.config_path) as f:
            return f.read()

    def ready(self):
        self.wait_log("Global hotkey Screenshot")
        # The X server applies the grab a moment after the log line.
        time.sleep(0.4)
        return self

    def stop(self):
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.logf.close()

    def keep_log(self, name):
        os.makedirs(ARTIFACTS, exist_ok=True)
        shutil.copy(self.log_path, os.path.join(ARTIFACTS, f"{name}.log"))


DEFAULT = """
[general]
notifications = "toast"
"""


# ───────────── tests ─────────────

def test_startup_registers_hotkeys_and_shows_a_popup(app):
    app.ready()
    log = app.log()
    assert "Global hotkey Region registered: Ctrl+Shift+S" in log, log
    assert "Global hotkey Screenshot registered: Ctrl+PrintScreen" in log, log
    assert "Tray icon created" in log, "tray failed to start: " + log
    wait_for(lambda: windows(POPUP_NAME), what="startup popup")
    w = windows(POPUP_NAME)[0]
    x, y, ww, hh = geometry(w)
    assert (ww, hh) == (380, 84), (ww, hh)
    assert x + ww <= W and y + hh <= H and x > W // 2 and y > H // 2, (x, y)


def test_one_press_captures_the_monitor_under_the_mouse(app):
    two_monitors(True)
    app.ready()
    wait_for(lambda: windows(POPUP_NAME), what="startup popup")
    move(1500, 500)  # right monitor
    clear_clipboard()
    key("ctrl+Print")
    files = wait_for(app.files, 5, what="saved file")
    img = Image.open(os.path.join(app.shots, files[0])).convert("RGB")
    assert img.size == (W // 2, H), img.size
    # Every corner is the right monitor's colour: the startup popup (which
    # sits in the bottom-right) was cleared before the capture.
    for xy in [(0, 0), (img.width - 1, 0), (0, img.height - 1), (img.width - 1, img.height - 1),
               (img.width - 120, img.height - 60)]:
        assert close(img.getpixel(xy), RIGHT_RGB), (xy, img.getpixel(xy))
    clip = wait_for(clipboard_png, 3, what="image on clipboard")
    assert clip.size == img.size, (clip.size, img.size)
    assert clip.getpixel((10, 10)) == img.getpixel((10, 10))
    wait_for(lambda: windows(POPUP_NAME), what="result popup")
    save_artifact(root_png(), "one-press-popup.png")
    # And on the left monitor:
    move(300, 500)
    time.sleep(0.6)
    key("ctrl+Print")
    files = wait_for(lambda: app.files() if len(app.files()) == 2 else None, 5, what="second file")
    img2 = Image.open(os.path.join(app.shots, files[1])).convert("RGB")
    assert img2.size == (W // 2, H) and close(img2.getpixel((5, 5)), LEFT_RGB), img2.getpixel((5, 5))


def test_all_monitors_option(app):
    two_monitors(True)
    app.ready()
    key("ctrl+Print")
    files = wait_for(app.files, 5, what="saved file")
    img = Image.open(os.path.join(app.shots, files[0])).convert("RGB")
    assert img.size == (W, H), img.size
    assert close(img.getpixel((10, 10)), LEFT_RGB) and close(img.getpixel((W - 10, 10)), RIGHT_RGB)


def test_quick_presses_never_overwrite(app):
    app.ready()
    for _ in range(3):
        key("ctrl+Print")
        time.sleep(0.35)
    files = wait_for(lambda: app.files() if len(app.files()) == 3 else None, 6, what="three files")
    assert len(set(files)) == 3, files


def test_holding_the_hotkey_captures_once(app):
    app.ready()
    sh("xdotool", "keydown", "ctrl", "keydown", "Print", "sleep", "1.0", "keyup", "Print", "keyup", "ctrl")
    wait_for(app.files, 5, what="a file")
    time.sleep(1.0)
    assert len(app.files()) == 1, app.files()


def test_region_drag_then_enter_copies(app):
    app.ready()
    clear_clipboard()
    key("ctrl+shift+s")
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    drag(200, 150, 600, 450)
    time.sleep(0.2)
    save_artifact(root_png(), "region-selected.png")
    key("Return")
    clip = wait_for(clipboard_png, 4, what="region on clipboard")
    assert abs(clip.size[0] - 400) <= 2 and abs(clip.size[1] - 300) <= 2, clip.size
    assert close(clip.getpixel((10, 10)), LEFT_RGB), clip.getpixel((10, 10))
    never(lambda: app.files(), 1.0, what="a file (copy_also_saves is off)")
    wait_for(lambda: windows(POPUP_NAME), what="Copied popup")


def test_region_escape_copies_nothing(app):
    app.ready()
    clear_clipboard()
    before = clipboard_hash()
    key("ctrl+shift+s")
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    drag(200, 150, 600, 450)
    key("Escape")  # deselect is only for annotations; this closes
    wait_for(lambda: not overlays(), what="overlay closed")
    time.sleep(0.5)
    assert clipboard_hash() == before
    assert app.files() == []


def test_finish_on_release_and_copy_also_saves(app):
    app.ready()
    clear_clipboard()
    key("ctrl+shift+s")
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    drag(1100, 200, 1400, 400)
    # No Enter: releasing the mouse finishes the capture.
    wait_for(lambda: not overlays(), 4, what="overlay to close by itself")
    files = wait_for(app.files, 5, what="saved file")
    img = Image.open(os.path.join(app.shots, files[0])).convert("RGB")
    assert abs(img.size[0] - 300) <= 2 and abs(img.size[1] - 200) <= 2, img.size
    assert close(img.getpixel((5, 5)), RIGHT_RGB)
    clip = wait_for(clipboard_png, 3, what="clipboard")
    assert clip.size == img.size


def test_rebinding_in_settings_takes_effect_immediately(app):
    app.ready()
    sw = open_settings_from_tray(app)
    click_in(sw, 269, 66)  # Keys tab
    time.sleep(0.3)
    click_in(sw, 280, 163)  # Screenshot row's key button
    app.wait_log("Global hotkeys suspended")
    time.sleep(0.2)
    key("ctrl+alt+p")
    time.sleep(0.3)
    save_artifact(root_png(), "settings-rebound.png")
    click_in(sw, 352, 526)  # Save & Close
    wait_for(lambda: not windows("screen-mark Settings"), what="settings to close")
    wait_for(lambda: 'screenshot = "Ctrl+Alt+P"' in app.config(), what="config written")
    app.wait_log("Global hotkey Screenshot registered: Ctrl+Alt+P")
    time.sleep(0.4)
    key("ctrl+Print")
    never(app.files, 1.2, what="the old hotkey to still capture")
    key("ctrl+alt+p")
    wait_for(app.files, 5, what="capture from the new hotkey")


def test_settings_can_capture_the_current_hotkey_itself(app):
    # While listening, the app's own grabs are released, so pressing the
    # current region hotkey binds it instead of opening the overlay.
    app.ready()
    sw = open_settings_from_tray(app)
    click_in(sw, 269, 66)
    time.sleep(0.3)
    click_in(sw, 280, 163)  # Screenshot row
    app.wait_log("Global hotkeys suspended")
    key("ctrl+shift+F12")
    time.sleep(0.3)
    click_in(sw, 280, 135)  # Region row
    time.sleep(0.2)
    key("ctrl+shift+s")
    time.sleep(0.5)
    assert not overlays(), "overlay opened while rebinding"
    click_in(sw, 352, 526)
    wait_for(lambda: 'screenshot = "Ctrl+Shift+F12"' in app.config(), what="config written")
    assert 'region = "Ctrl+Shift+S"' in app.config(), app.config()
    app.wait_log("Global hotkey Screenshot registered: Ctrl+Shift+F12")
    time.sleep(0.4)  # the grab reaches the X server on the hotkey thread's next poll
    key("ctrl+shift+F12")
    wait_for(app.files, 5, what="capture from Ctrl+Shift+F12")


def test_a_taken_hotkey_is_reported_and_fixable_from_the_popup(app_factory):
    grabber = subprocess.Popen([sys.executable, "-c", textwrap.dedent("""
        import time
        from Xlib import X, XK, display
        d = display.Display()
        root = d.screen().root
        code = d.keysym_to_keycode(XK.string_to_keysym("Print"))
        root.grab_key(code, X.ControlMask, False, X.GrabModeAsync, X.GrabModeAsync)
        d.sync()
        print("grabbed", flush=True)
        time.sleep(60)
    """)], stdout=subprocess.PIPE, text=True)
    try:
        assert grabber.stdout.readline().strip() == "grabbed"
        app = app_factory(DEFAULT)
        app.wait_log("Screenshot")
        log = app.log()
        assert "Taken" in log, log
        assert "Global hotkey Region registered" in log, log
        popup = wait_for(lambda: windows(POPUP_NAME), what="error popup")[0]
        save_artifact(root_png(), "taken-popup.png")
        # The region hotkey still works.
        # Clicking the popup opens Settings on the Keys tab.
        click_in(popup, 200, 40)
        sw = wait_for(lambda: windows("screen-mark Settings"), what="settings")[0]
        time.sleep(0.4)
        save_artifact(root_png(), "taken-settings.png")
        click_in(sw, 280, 163)
        app.wait_log("Global hotkeys suspended")
        key("ctrl+Print")  # still taken: refused, keeps listening
        time.sleep(0.3)
        key("ctrl+alt+p")
        time.sleep(0.3)
        click_in(sw, 352, 526)
        wait_for(lambda: 'screenshot = "Ctrl+Alt+P"' in app.config(), what="config written")
        app.wait_log("Global hotkey Screenshot registered: Ctrl+Alt+P")
        # Saving must not knock out the region hotkey (it used to report
        # "taken" by our own just-released grab).
        assert "Global hotkey Region 'Ctrl+Shift+S': Taken" not in app.log(), app.log()
        time.sleep(0.4)
        key("ctrl+alt+p")
        wait_for(app.files, 5, what="capture")
        key("ctrl+shift+s")
        wait_for(overlays, what="region hotkey still working after Save")
        key("Escape")
    finally:
        grabber.kill()


def test_notifications_off_still_shows_errors(app):
    app.ready()
    time.sleep(0.3)
    never(lambda: windows(POPUP_NAME), 0.8, what="startup popup with notifications off")
    key("ctrl+Print")
    wait_for(app.files, 5, what="file")
    never(lambda: windows(POPUP_NAME), 1.0, what="success popup with notifications off")


def test_unwritable_folder_still_copies_and_says_so(app):
    app.ready()
    clear_clipboard()
    key("ctrl+Print")
    wait_for(clipboard_png, 4, what="clipboard despite the bad folder")
    wait_for(lambda: windows(POPUP_NAME), what="error popup")
    save_artifact(root_png(), "unwritable-popup.png")
    assert "Screenshot" in app.log()


def dbus_python():
    """A Python that can import dbus (the distro's bindings are built for
    its own python3, which may not be the first python3 on PATH)."""
    for exe in [sys.executable, "/usr/bin/python3", "/usr/bin/python3.12", "/usr/bin/python3.11"]:
        if os.path.exists(exe) and subprocess.run(
                [exe, "-c", "import dbus, gi"], capture_output=True).returncode == 0:
            return exe
    raise AssertionError("no python with dbus-python and PyGObject")


def test_system_notifications_go_to_dbus(app_factory):
    received = os.path.join(tempfile.mkdtemp(), "notify.txt")
    server = subprocess.Popen([dbus_python(), "-c", textwrap.dedent(f"""
        import dbus, dbus.service, dbus.mainloop.glib
        from gi.repository import GLib
        dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
        bus = dbus.SessionBus()
        name = dbus.service.BusName("org.freedesktop.Notifications", bus)
        class N(dbus.service.Object):
            @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{{sv}}i", out_signature="u")
            def Notify(self, app, rid, icon, summary, body, actions, hints, timeout):
                open({received!r}, "a").write(f"{{app}}|{{summary}}|{{body}}\\n")
                return 1
            @dbus.service.method("org.freedesktop.Notifications", out_signature="as")
            def GetCapabilities(self):
                return ["body"]
            @dbus.service.method("org.freedesktop.Notifications", out_signature="ssss")
            def GetServerInformation(self):
                return ("fake", "e2e", "1", "1.2")
        N(bus, "/org/freedesktop/Notifications")
        print("ready", flush=True)
        GLib.MainLoop().run()
    """)], stdout=subprocess.PIPE, text=True)
    try:
        assert server.stdout.readline().strip() == "ready"
        app = app_factory('[general]\nnotifications = "system"\n')
        app.ready()
        key("ctrl+Print")
        wait_for(app.files, 5, what="file")
        text = wait_for(lambda: os.path.exists(received) and open(received).read(), 5,
                        what="a D-Bus notification")
        assert "screen-mark|Saved and copied|" in text, text
        never(lambda: windows(POPUP_NAME), 0.5, what="a popup in system mode")
    finally:
        server.kill()


def test_clicking_the_popup_shows_the_file_in_its_folder(app):
    app.ready()
    key("ctrl+Print")
    wait_for(app.files, 5, what="file")
    popup = wait_for(lambda: windows(POPUP_NAME), what="popup")[0]
    time.sleep(0.2)
    click_in(popup, 200, 40)
    wait_for(lambda: os.path.exists(app.opened), 3, what="xdg-open")
    assert app.shots in open(app.opened).read()
    wait_for(lambda: not windows(POPUP_NAME), 2, what="popup to close on click")


def test_a_2_0_config_still_works(app):
    # 2.0 wrote [hotkey] capture = ...; it must keep working.
    app.wait_log("Global hotkey Region registered: Ctrl+Alt+R")
    key("ctrl+alt+r")
    wait_for(overlays, what="overlay from the 2.0 binding")
    key("Escape")


def test_cli_interactive_capture_exits_when_done(app_factory):
    app = app_factory(DEFAULT, args=("capture",))
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    key("Escape")
    wait_for(lambda: app.proc.poll() is not None, 4, what="the process to exit")
    assert app.proc.returncode == 0, app.proc.returncode


def test_tray_menu_screenshot_item(app):
    app.ready()
    tray_menu(app, 2)  # "Screenshot (save + copy)"
    wait_for(app.files, 5, what="file from the tray item")


def test_editor_keys_follow_the_config(app):
    # Copy rebound to Ctrl+Alt+C; Ctrl+C now does nothing in the editor.
    app.ready()
    clear_clipboard()
    before = clipboard_hash()
    key("ctrl+shift+s")
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    drag(200, 150, 500, 350)
    time.sleep(0.2)
    key("ctrl+c")
    time.sleep(0.6)
    assert overlays(), "Ctrl+C closed the overlay although Copy was rebound"
    assert clipboard_hash() == before
    key("ctrl+alt+c")
    clip = wait_for(clipboard_png, 4, what="copy via Ctrl+Alt+C")
    assert abs(clip.size[0] - 300) <= 2


def test_a_bound_ctrl_enter_runs_its_action_not_the_fixed_copy(app):
    # Enter copies; Save rebound to Ctrl+Enter must save instead.
    app.ready()
    clear_clipboard()
    before = clipboard_hash()
    key("ctrl+shift+s")
    wait_for(overlays, what="overlay")
    time.sleep(0.3)
    drag(200, 150, 500, 350)
    time.sleep(0.2)
    key("ctrl+Return")
    wait_for(app.files, 5, what="file saved by Ctrl+Enter")
    time.sleep(0.4)
    assert clipboard_hash() == before, "Ctrl+Enter copied (the fixed Enter path ran)"


def test_a_2_0_one_modifier_hotkey_still_registers(app):
    # 2.0 let people pick Alt+letter; 2.1 only cautions about it.
    app.wait_log("Global hotkey Region registered: Alt+S")
    assert "isn't working" not in app.log(), app.log()
    time.sleep(0.4)  # the grab reaches the X server on the hotkey thread's next poll
    key("alt+s")
    wait_for(overlays, what="overlay from Alt+S")
    key("Escape")


def test_switching_away_while_listening_brings_the_hotkeys_back(app):
    app.ready()
    sw = open_settings_from_tray(app)
    click_in(sw, 269, 66)  # Keys tab
    time.sleep(0.3)
    click_in(sw, 280, 135)  # Region row: listening, hotkeys suspended
    app.wait_log("Global hotkeys suspended")
    other = subprocess.Popen(["display", "-title", "e2e-other", "-size", "200x200", "xc:gray"])
    try:
        win = wait_for(lambda: windows("e2e-other"), 5, what="another window")[0]
        sh("xdotool", "windowactivate", "--sync", win, check=False)
        app.wait_log("Global hotkeys resumed")
        time.sleep(0.4)
        key("ctrl+shift+s")
        wait_for(overlays, what="overlay after switching away")
        key("Escape")
    finally:
        other.kill()


# ───────────── tray and settings helpers ─────────────

def tray_menu(app, index):
    """Open the tray menu and activate the index-th item (1-based)."""
    icon = wait_for(lambda: [w for w in windows("screen-mark") if geometry(w)[2] <= 32], 5,
                    what="tray icon")[0]
    x, y, w, h = geometry(icon)
    sh("xdotool", "mousemove", str(x + w // 2), str(y + h // 2), "sleep", "0.2", "click", "1")
    time.sleep(0.6)
    for _ in range(index):
        sh("xdotool", "key", "Down")
        time.sleep(0.05)
    sh("xdotool", "key", "Return")


def open_settings_from_tray(app):
    tray_menu(app, 10)  # Settings
    sw = wait_for(lambda: windows("screen-mark Settings"), 5, what="settings window")[0]
    sh("xdotool", "windowactivate", "--sync", sw, check=False)
    sh("xdotool", "windowfocus", "--sync", sw, check=False)
    time.sleep(0.4)
    return sw


# ───────────── runner ─────────────

CASES = [
    (test_startup_registers_hotkeys_and_shows_a_popup, DEFAULT),
    (test_one_press_captures_the_monitor_under_the_mouse, DEFAULT),
    (test_all_monitors_option, '[general]\nscreenshot_area = "all"\n'),
    (test_quick_presses_never_overwrite, DEFAULT),
    (test_holding_the_hotkey_captures_once, DEFAULT),
    (test_region_drag_then_enter_copies, DEFAULT),
    (test_region_escape_copies_nothing, DEFAULT),
    (test_finish_on_release_and_copy_also_saves,
     '[general]\nfinish_on_release = true\ncopy_also_saves = true\n'),
    (test_rebinding_in_settings_takes_effect_immediately, DEFAULT),
    (test_settings_can_capture_the_current_hotkey_itself, DEFAULT),
    (test_a_taken_hotkey_is_reported_and_fixable_from_the_popup, None),
    (test_notifications_off_still_shows_errors, '[general]\nnotifications = "off"\n'),
    (test_unwritable_folder_still_copies_and_says_so,
     '[general]\nnotifications = "off"\nsave_directory = "/proc/version/nope"\n'),
    (test_system_notifications_go_to_dbus, None),
    (test_clicking_the_popup_shows_the_file_in_its_folder, DEFAULT),
    (test_a_2_0_config_still_works, '[hotkey]\ncapture = "Ctrl+Alt+R"\n'),
    (test_cli_interactive_capture_exits_when_done, None),
    (test_tray_menu_screenshot_item, DEFAULT),
    (test_editor_keys_follow_the_config, '[keys]\ncopy = "Ctrl+Alt+C"\n'),
    (test_a_bound_ctrl_enter_runs_its_action_not_the_fixed_copy,
     '[general]\nask_where_to_save = false\n[keys]\nsave = "Ctrl+Enter"\n'),
    (test_a_2_0_one_modifier_hotkey_still_registers, '[hotkey]\ncapture = "Alt+S"\n'),
    (test_switching_away_while_listening_brings_the_hotkeys_back, DEFAULT),
]


def main():
    paint_background()
    passed, failed = [], []
    for fn, cfg in CASES:
        name = fn.__name__
        if ONLY and ONLY not in name:
            continue
        two_monitors(False)
        apps = []

        def factory(toml, extra_env=None, args=()):
            a = App(toml, extra_env, args)
            apps.append(a)
            return a

        t0 = time.time()
        try:
            if cfg is None:
                fn(factory)
            else:
                fn(factory(cfg))
            passed.append(name)
            print(f"ok    {name} ({time.time() - t0:.1f}s)", flush=True)
        except Exception as e:  # noqa: BLE001
            failed.append(name)
            print(f"FAIL  {name}: {e}", flush=True)
            traceback.print_exc()
            try:
                save_artifact(root_png(), f"FAIL-{name}.png")
            except Exception:  # noqa: BLE001
                pass
            for a in apps:
                a.keep_log(f"FAIL-{name}")
        finally:
            for a in apps:
                a.stop()
                shutil.rmtree(a.root, ignore_errors=True)
            # Let popups and grabs go away before the next case.
            subprocess.run(["xdotool", "key", "Escape"], capture_output=True)
            time.sleep(0.3)
    two_monitors(False)
    print(f"\n{len(passed)} passed, {len(failed)} failed")
    if failed:
        print("failed: " + ", ".join(failed))
        print(f"screenshots and logs: {ARTIFACTS}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
