# Contributing

One maintainer, one tool. Small, specific changes land fastest.

## Before you start

Open an issue first for anything bigger than a bug fix, so we agree on the shape before you write it. Check [Known issues](README.md#known-issues) and the open issues; the thing you hit may already be there.

## Building

```sh
git clone https://github.com/Real-Fruit-Snacks/screen-mark
cd screen-mark
cargo build
cargo run                       # debug builds never self-install
```

Linux needs `libxkbcommon-dev libwayland-dev libglib2.0-dev libgtk-3-dev libxdo-dev` (the same list CI installs).

## Before you open a PR

CI runs these on Windows and Linux and fails on any warning:

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

- Add a test under `tests/` for behaviour you changed. Most modules already have one.
- If you touched hotkeys, popups, the tray, Settings, or the capture flow, run the end-to-end tests too. They start the real binary on a virtual X display (Linux only) and need a few extra packages:

  ```sh
  sudo apt-get install -y xvfb openbox stalonetray xdotool xclip imagemagick x11-xserver-utils \
      x11-utils python3-pil python3-xlib python3-dbus python3-gi dbus-x11 libayatana-appindicator3-1
  cargo build
  tests/e2e/run.sh                      # all scenarios
  tests/e2e/run.sh target/debug/screen-mark rebind   # just the ones whose name contains "rebind"
  ```

- Add a line to `CHANGELOG.md` under an `Unreleased` heading.
- Update the README if a command, key, or config key changed.
- Keep platform-specific code behind `cfg` in `src/capture/` or the module that already owns it.

## Commits and PRs

Imperative mood, present tense, first line under 72 characters: `Fix HiDPI offset in pinned windows`. Reference issues in the body, not the title. One concern per PR.

## Conduct

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies. Report problems through the security advisory link if you need privacy, or by email to the address on my profile.
