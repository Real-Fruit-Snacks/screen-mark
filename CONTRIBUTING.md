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
- Add a line to `CHANGELOG.md` under an `Unreleased` heading.
- Update the README if a command, key, or config key changed.
- Keep platform-specific code behind `cfg` in `src/capture/` or the module that already owns it.

## Commits and PRs

Imperative mood, present tense, first line under 72 characters: `Fix HiDPI offset in pinned windows`. Reference issues in the body, not the title. One concern per PR.

## Conduct

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies. Report problems through the security advisory link if you need privacy, or by email to the address on my profile.
