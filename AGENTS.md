# AGENTS.md — screen-mark

Repository contract for contributors and coding agents working on `Real-Fruit-Snacks/screen-mark`.

## Brand System — Cyclorama Dawn `v2.1.0`

Source of truth: `Real-Fruit-Snacks/rfs-brand` (`v2.1.0`).

- **Theme Modes**: Matte Dark `LX 263` (`#050507`) is the explicit default (`<meta name="color-scheme" content="dark">`). Alabaster Day `LX 512` (`#F5F7F9`) is an opt-in toggle synchronized across tabs via `localStorage['rfs-theme']` with a radial View Transition.
- **Palette Tokens**:
  - Surfaces (Dark): `#050507` bg, `#0D0E12` surface, `#161820` raised, `#242834` line, `#5D6476` strong line, `#F2F4F8` text, `#8F95A6` mute.
  - Surfaces (Light): `#F5F7F9` bg, `#FFFFFF` surface, `#E9EEF3` raised, `#CBD3DD` line, `#76808F` strong line, `#11141B` text, `#535B69` mute.
  - Accents: Glacial Steel (`#1F547A` / `#74B8E8` / `#164669`), Maritime Beacon Orange (`#F06E2B` / `#BD4811` / `#FF9254` / `#AD3D0A`), Signal Crimson Fault (`#FF5C6C` / `#C92438` dark; `#B91C2E` light), Mint (`#3DDC97` / `#15794D`), Amber (`#FFC247` / `#885800`).
  - Gradients: Cartesian `in oklab` only (never `in oklch`). Zero neon glows, zero fake macOS traffic-light dots, zero `99px` capsule pills.
- **Typography**: Self-hosted variable WOFF2 only (`Big Shoulders Stencil`, `Albert Sans`, `Azeret Mono`, plus `OFL.txt`) in `docs/assets/fonts/`. Zero external CDNs. Keep `font-variant-ligatures: none` on `Azeret Mono` so `(R)` and `(C)` stay literal.
- **Scroll Shell**: `<body class="rfs-shell">` + pinned `<header class="rfs-shell-header">` + `<div class="rfs-scroll-region">` so the vertical scrollbar never enters the top header bar. Never animate `scrollbar-color`.
- **Voice**: Plain, specific, honest — zero emoji, zero exclamation marks in headings or descriptions, zero hype copy.

## Rust App & Website Invariants

- `src/theme.rs` and `tests/theme_tests.rs` pin the Cyclorama Dawn `v2.1.0` palette and enforce strict WCAG AA (`>= 4.5:1`) for every foreground token on both `bg_0` and `bg_1` in Dark and Light modes.
- `src/geometry.rs` pins the 5 annotation presets (`#ff5c6c`, `#74b8e8`, `#3ddc97`, `#ffc247`, `#b78cff`); update `src/overlay/toolbar.rs`, `docs/index.html`, `docs/art/make-panel.py`, and `tests/markup_2_4_tests.rs` together if presets change.
- `examples/generate_icon.rs` regenerates `assets/icon.png` (embedded into `assets/icon.ico` by `build.rs` and loaded by `src/tray.rs`) and `docs/icon_256.png`.
- `docs/art/make-panel.py` generates `docs/assets/panel-dark.svg` and `docs/assets/panel-light.svg`; CI checks `git diff --exit-code` on both files.

## Verification

Before pushing:

```sh
cargo fmt --check
cargo test
cargo clippy -- -D warnings
python docs/art/make-panel.py
git diff --exit-code -- docs/assets/panel-dark.svg docs/assets/panel-light.svg
```
