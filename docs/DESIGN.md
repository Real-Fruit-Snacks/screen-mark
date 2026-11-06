# Design: screen-mark website

## Concept
The page is a screen that screen-mark is in the middle of capturing. The first screen sits under the
app's real capture overlay: dimmed (40% black as in the app; on Dayshift 30% of the warm ink colour,
because paper under 40% black reads as a disabled page), an Ember selection around the download block with its live `W × H` readout, the
24-button toolbar under it (above it when the bottom of the screen is in the way and the name isn't, as
the app does), and an arrow already drawn. The download button itself is never dimmed, and the name
stays lit while the selection is the one on the download block; a selection of your own dims it like
the rest. On Nightshift the stage sits on `--rfs-surface` so the dim visibly darkens it; the stage
ends on a hairline. Visitors can
drag their own selection and draw on the page with the app's keys. Everything below the first screen is plain,
specific and set like a well-made manual. The visitor should leave knowing the three hotkeys and the
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
  commands and the toolbar's size pill. Its ligatures are off everywhere: they turn `(R)` into ® and
  join the fi in `config`. It's wide and screen-like, and its numerals stay legible at small sizes.
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
| accent (Amber) | `--rfs-accent` | The quote's highlighter band. Annotations and step markers (hero and Install) use `#FFC247`, Nightshift's accent and the app's yellow preset, in both schemes |

Rules: Ember means "capture/selection" or "the one action" and nothing else. Annotations are the app's yellow.
The toolbar's five swatches are the app's real presets (`#ff6e7a #5cb8ff #3ddc97 #ffc247 #b78cff`),
because the toolbar depicts the shipping product; these are the only non-RFS values on the page.
No gradients.

Text left outside the selection on the first screen is dimmed by the overlay, so it must use `text`
(not `mute`) to stay AA under the dim. Focus rings there sit on a halo of page colour so they read
against the dim.

Annotations, the hero arrow and step marker 1 are drawn in the app's yellow in both schemes, as in a
real screenshot; on Dayshift they get a thin dark edge so they hold up on paper. The highlighter draws
at 40% opacity: a web page can't multiply into the pixels under it, so it is see-through instead.
While the download block is outside the selection, its muted text switches to `text`.

## Layout
One column at 72rem max with a 12-column grid inside. The first screen is a full-height "stage"
(header, wordmark, download block). Sections then vary by content:
- hotkeys: an asymmetric split with the key caps set at display size;
- tools: a full-width QWERTY diagram;
- after the capture: a dense definition list;
- the maintainer's reason, set large;
- install: three numbered steps beside the files, the installer as the section's one button, then
  one command and the CLI table at a smaller size;
- tested on and known issues: two plain lists.
Sections are separated by space and a hairline, never by tinted bands.

## Signature
1. The first screen is under the real capture overlay, and it works.
2. The QWERTY diagram with only the fourteen tool keys lit. The lit keys are buttons: click or tap one,
   or press its letter while focus is in the diagram, and it lights up and switches the tool on the
   first screen. A caption under it names the current tool.

## Wow moment
On load (about 0.7s once the fonts are in): the stage dims, the crosshair lands on the download block's corner, the
selection drags open while the readout counts up in device pixels, the toolbar appears beneath it, an
Amber arrow draws toward the download button and step marker 1 drops beside it. The headline is
readable from the first frame; only the overlay animates.

Then it is live (mouse and trackpad): drag anywhere to select, click to snap to a block (the app's
"click for a window"), draw inside the selection with arrow, rectangle, circle, rounded rectangle,
line, pencil, highlight, spotlight, step marker and measurement. `Ctrl+Z` / `Ctrl+Shift+Z` undo and
redo, `Esc` clears and `Enter` selects the download block again. Once a tool has been picked, a
click without a drag starts a shape that follows the pointer until the next click (before that, a
stray click draws nothing). Copy, Save, Pin, Upload and OCR answer in the
app's corner toast and say plainly that a web page can't do that. Save shows the file name the app
would write, with the current time. Text, pixelate and eyedropper do the same.

The tool letters, `Esc`, `Enter` and the undo keys act only while focus is in the first screen (the
letters also in the key diagram, and never on a link), never page-wide. A click on the first screen
focuses it without a ring; `Esc` acts from the screen or its toolbar, and from the toolbar it brings
focus back to the screen, where `Enter` reselects the download block. The toolbar is one tab
stop with arrow keys; activating a tool from the keyboard draws an example of its shape. The toast
never takes clicks; it sits in the bottom corner of what's visible of the stage, narrows to fit beside
the toolbar and the download block, and only moves to the top when neither works. On touch it sits
in the viewport just below the toolbar, clear of the download block's links. It hides when it would cover the focused element, and every message also goes to a status region for
screen readers. The first hint shows only where it covers nothing, and goes away on the first click
or key; a mouse click on a tool says how to draw with it.

- **No JS, or a script that never arrives:** if `script.js` hasn't started within 4s the page drops
  the `js` class and shows the same still frame. The selection outline and dim are drawn by CSS around
  the download block, the toolbar
  sits under it, and the arrow and step marker are static SVG. A finished still frame.
- **Reduced motion:** the same final frame, with no entrance. Interaction still works.
- **Phone / touch:** the final frame, with a shortened toolbar (the app's `[toolbar]` config can
  hide tools too). Dragging is left to the page scroll; tapping a tool draws an example of its shape, replacing the last one (touch has no undo).
  In portrait the stage is only as tall as its content, so the next section shows under it, below
  a hairline. The size readout counts CSS pixels on touch screens. Below
  360px the key diagram gives way to the list. Landscape phones get a smaller name and a wider lede
  so the button stays on the first screen.

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
