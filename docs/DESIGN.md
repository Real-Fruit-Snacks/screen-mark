# Design: screen-mark website

## Concept
The page is a screen that screen-mark is in the middle of capturing. The first screen sits under the
app's real capture overlay: dimmed 40%, an Ember selection around the download block with its live
`W × H` readout, the 24-button toolbar under it, and an arrow already drawn. Visitors can drag their
own selection and draw on the page with the app's keys. Everything below the first screen is plain,
specific and set like a well-made manual. The visitor should leave knowing the two hotkeys and the
fourteen tool keys, and having already used the overlay once.

## Audience and primary action
Windows users (and some Linux/X11 users) who take screenshots for tickets, docs and chat, and are tired
of slow or bloated capture tools. They need to know what it does, what it runs on, and that it is a
single native exe. Primary action: **Download for Windows** (the latest release). Secondary: the Linux
tarball and `cargo install`.

## Type
- Display and text: **Bricolage Grotesque** (variable, `opsz` 12–96, `wght` 400–800), self-hosted.
  At display sizes the optical-size axis gives compressed, heavy letters with ink traps, so
  "screen-mark" can fill the stage. At text size the same family is calm and open. IBM Plex is the
  RFS house face, so it is deliberately not used here.
- Mono: **Azeret Mono** (`wght` 400–600), self-hosted. Used for key caps, readouts, file names,
  commands and the toolbar's size pill. It's wide and screen-like, and its numerals stay legible at small sizes.
- Scale: `scripts/scale.mjs` with 16→18px base and ratio 1.2→1.333. The wordmark and the keycaps in the
  hotkeys section use hand-written clamps beyond the scale. Nothing is set below 12px.

## Color
The RFS Nightshift / Dayshift color tokens, pasted verbatim from `rfs-brand/brand/tokens/rfs.css`
(v1.2.0). Only the colors come from RFS: no Plex, no dot grid, no RFS layout or mark. Dark or light
follows the system setting.

| Role | Token | Use |
|---|---|---|
| bg | `--rfs-bg` | Page |
| surface | `--rfs-surface` | Toolbar, toast, key caps |
| raised | `--rfs-raised` | Unlit keys, code |
| line / line-strong | `--rfs-line`, `--rfs-line-strong` | Rules; key and control borders |
| text / mute | `--rfs-text`, `--rfs-mute` | Copy |
| primary (Ember) | `--rfs-primary` (+ hover, press, on-primary) | The capture selection, the crosshair, the download button, focus rings |
| accent (Amber) | `--rfs-accent` | Annotations drawn on the page (the app's "yellow" preset) |

Rules: Ember means "capture/selection" or "the one action" and nothing else. Annotations are Amber.
The toolbar's five swatches are the app's real presets (`#ff6e7a #5cb8ff #3ddc97 #ffc247 #b78cff`),
because the toolbar depicts the shipping product; these are the only non-RFS values on the page.
No gradients.

Text left outside the selection on the first screen is dimmed by the overlay, so it must use `text`
(not `mute`) to stay AA under the 40% black.

## Layout
One column at 72rem max with a 12-column grid inside. The first screen is a full-height "stage"
(header, wordmark, download block). Sections then vary by content:
- hotkeys: an asymmetric split with the key caps set at display size;
- tools: a full-width QWERTY diagram;
- after the capture: a dense definition list;
- the maintainer's reason, set large;
- install: a file list and one command;
- tested on and known issues: two plain lists.
Sections are separated by space and a hairline, never by tinted bands.

## Signature
1. The first screen is under the real capture overlay, and it works.
2. The QWERTY diagram with only the fourteen tool keys lit. Pressing a key on your keyboard lights
   it and switches the tool on the first screen.

## Wow moment
On load (about 1.3s): the stage dims, the crosshair lands on the download block's corner, the
selection drags open while the readout counts up in device pixels, the toolbar appears beneath it, an
Amber arrow draws toward the download button and step marker 1 drops beside it. The headline is
readable from the first frame; only the overlay animates.

Then it is live (mouse and trackpad): drag anywhere to select, click to snap to a block (the app's
"click for a window"), draw inside the selection with arrow, rectangle, circle, rounded rectangle,
line, pencil, highlight, spotlight, step marker and measurement. `Ctrl+Z` / `Ctrl+Shift+Z` undo and
redo, `Esc` clears. Copy, Save, Pin, Upload and OCR answer in the app's corner toast and say plainly
that a web page can't do that. Save shows the file name the app would write, with the
current time. Text, pixelate and eyedropper do the same.

- **No JS:** the selection outline and dim are drawn by CSS around the download block, the toolbar
  sits under it, and the arrow and step marker are static SVG. A finished still frame.
- **Reduced motion:** the same final frame, with no entrance. Interaction still works.
- **Phone / touch:** the final frame, with a shortened toolbar (the app's `[toolbar]` config can
  hide tools too). Dragging is left to the page scroll.

## Imagery and texture
No photos, no illustration. The visuals are the app's own interface rebuilt in HTML/CSS/SVG at true
proportions: the selection (1.5px Ember), the size pill, the 40px toolbar with Lucide icons (the
same paths the app renders), the 380×84 corner toast, and the key caps. No texture.

## Motion
Only the first screen's entrance and its interactions move. Elsewhere there are 150ms hover and
press transitions, and the highlighter mark in the quote, which draws in as it scrolls into view
(scroll-driven, CSS only). Everything is inside `prefers-reduced-motion: no-preference` or a
`matchMedia` check.

## Voice
First person singular, plain, specific, dry. Real key names, file names, versions and limits. Say
what it doesn't do. No exclamation marks, no emoji, no "powerful/seamless/modern/blazing".

## Maintenance
- **On every release:** the version appears in the hero's download link and meta line, the three
  file links and names in Install, and the footer. Search `index.html` for the old version.
- **Link preview:** `assets/og.png` is the first screen rendered by Chromium at 1600×840 and 0.75
  scale (1200×630), in dark mode, after the entrance, with the popup hidden. Re-render it if the hero
  changes.
- **Icons:** `icon.svg` and `favicon.ico` use a reduced mark (corner brackets and a heavier
  arrow on a 32-unit grid) so they read at 16px. The PNG app icons use the full mark.

## Alternatives considered
- **Marked-up bug report:** the page as a ticket with annotated screenshots attached, set in a
  slab serif. It's honest, but it shows the tool's output rather than letting visitors use it.
- **Contact sheet:** the page as a wall of past captures from the History window, with crop marks
  and frame numbers. It's striking, but it would need real screenshots of real work that don't exist yet.
