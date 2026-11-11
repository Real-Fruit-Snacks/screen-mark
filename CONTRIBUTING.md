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

Rust 1.88 or later (`rust-version` in `Cargo.toml`; CI builds with exactly that version). Linux needs `libxkbcommon-dev libwayland-dev libglib2.0-dev libgtk-3-dev libxdo-dev` (the same list CI installs).

## Before you open a PR

CI runs these on Windows and Linux and fails on any warning:

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

- Add a test under `tests/` for behaviour you changed. Most modules already have one. The overlay, Settings and History are driven without a window (`tests/overlay_input_tests.rs`, `tests/settings_view_tests.rs`, `tests/history_view_tests.rs`), and the render tests write snapshots to `target/overlay-*.png` and `target/settings-*.png` to look at.
- A new annotation tool is one file in `src/tools/` plus an entry in each list that `tests/redact_tests.rs` checks against the others. Give it an arm in `tools::annotation_reach` too: the overlay redraws only the areas a frame says it drew on, and `tests/overlay_frame_tests.rs` fails if an annotation draws outside the area it claims.
- If you touched `src/renderer.rs`, run `cargo run --release --example overlay_bench` before and after: it times a frame at 4K and at two 4K screens.
- The OCR tests that call Windows OCR are opt-in: `cargo test --test ocr_tests -- --ignored` (Windows, with an OCR language installed).
- If you touched Settings or History, check them with a screen reader or `cargo run --example access_probe` (Windows).
- If you touched `src/update/`, `tests/update_tests.rs` runs whole updates against release folders on disk, signed with keys made up for the test; the "program" in them is a script that answers `--version`. For the real thing, `screen-mark update --from <folder>` on a release build takes any folder that holds a signed `latest.json` and the downloads.
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

## Releases and the update key

Pushing a `v*` tag runs `release.yml`. Besides the downloads it writes `latest.json` (the version, and each download's size and SHA-256) and signs it into `latest.json.sig`. An installed screen-mark updates only from a manifest signed with one of the two Ed25519 keys in `src/update/keys.rs`:

- The **release key** signs every release. Its private half is the `UPDATE_SIGNING_KEY` repository secret (Settings › Secrets and variables › Actions), as 64 hex characters.
- The **recovery key** is kept offline and never given to CI. If the release key is lost or leaks, a release signed with the recovery key still reaches every installed copy, and can carry a new release key.

A tagged release stops before publishing if the secret is missing, or if it is not one of the keys in `keys.rs`. Running the workflow by hand without the secret only warns.

```sh
bash .github/scripts/update-manifest.sh keygen ~/release.key   # a new key; prints the public half for keys.rs
UPDATE_SIGNING_KEY=$(cat ~/release.key) bash .github/scripts/update-manifest.sh public
bash .github/scripts/update-manifest.sh verify dist            # is dist/latest.json.sig from a trusted key?
```

To change a key, add the new public key to `keys.rs` and ship that release signed with a key the installed copies already trust. Remove the old key a release or two later: a copy can only move to a release its own keys accept. A fork that publishes its own releases needs its own two keys in `keys.rs`.

## Conduct

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies. Report problems through the security advisory link if you need privacy, or by email to the address on my profile.
