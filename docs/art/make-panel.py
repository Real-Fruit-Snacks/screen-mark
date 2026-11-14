"""Draw the README's animated panel: docs/assets/panel-dark.svg and panel-light.svg.

The panel acts out one capture in a 12 s loop, the way the app does it: Ctrl+Shift+S dims the
screen, a region is dragged out with its live W x H readout, the toolbar comes up, an arrow (A),
a box (R) and a step marker (N) go on, and Ctrl+C copies it, closes the overlay and shows the
app's "Copied" popup. The strip underneath lights each key as it is pressed.

The SVG's base styles are the finished capture (selection, marks and toolbar, before the copy),
which is what a viewer that skips the animation shows.

Text is converted to paths from the page's own fonts (docs/assets/fonts), because an SVG shown
through <img> can't load fonts. Run from the repo root:

    python -m pip install -r docs/art/requirements.txt
    python docs/art/make-panel.py
"""
import math
import os
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont

HERE = os.path.dirname(os.path.abspath(__file__))
FONTS = os.path.join(HERE, "..", "assets", "fonts")
OUT = os.path.join(HERE, "..", "assets")

# RFS Nightshift and Dayshift; the overlay's dim is the app's (40% black), on Dayshift 30% of the
# warm ink, as on the website (docs/DESIGN.md).
THEMES = {
    "dark": dict(bg="#0C0D10", plate="#1A1D24", raised="#22262E", line="#262A33", strong="#363B46", ink="#E8E6E1",
                 mute="#8A8F99", ember="#FF6A3D", on_ember="#0C0D10", mint="#3DDC97", dim="#000000", dim_a=0.40),
    "light": dict(bg="#F6F5F2", plate="#FFFFFF", raised="#ECEAE4", line="#D6D3CB", strong="#BDB9AF", ink="#1A1B1E",
                  mute="#666A73", ember="#C03B0A", on_ember="#FFFFFF", mint="#15794D", dim="#1A1B1E", dim_a=0.30),
}
# The app's five annotation presets: red, blue, green, yellow, mauve.
PRESETS = ["#ff6e7a", "#5cb8ff", "#3ddc97", "#ffc247", "#b78cff"]
RED, BLUE, YELLOW = 0, 1, 3

W, H = 1000, 520
CYCLE = 12.0  # seconds

# The screen being captured, and the capture on it.
SCREEN = (24, 24, 952, 400)              # x, y, w, h
SEL0, SEL1 = (500, 100), (812, 288)       # where the drag starts and ends: 312 x 188
ARROW = ((556, 140), (656, 190))          # tail, tip
BOX = (664, 132, 46, 132)                 # x, y, w, h: around the tallest bar
MARK = (734, 132)                         # step marker centre
TOOLS = "VARCLPHNT"                       # a slice of the 24-button toolbar
# The story, in seconds.
T_KEY, T_DIM, T_DRAG, T_SEL, T_BAR = 0.4, 0.8, 1.2, 2.4, 2.7
T_ARROW, T_ARROW_END = 3.1, 3.9
T_R, T_BOX, T_BOX_END = 4.3, 4.5, 5.1
T_N, T_MARK = 5.5, 5.8
T_COPY, T_CLOSE, T_TOAST_END = 6.6, 6.8, 9.8
DRAG_FRAMES = 12

_fonts = {}
_glyphs = {}   # (font, location, glyph name) -> (id, path in font units); drawn once in <defs>


def font(name):
    if name not in _fonts:
        _fonts[name] = TTFont(os.path.join(FONTS, name))
    return _fonts[name]


SANS = ("bricolage-grotesque-latin.woff2", (("opsz", 14), ("wght", 600)))
SANS_R = ("bricolage-grotesque-latin.woff2", (("opsz", 14), ("wght", 450)))
MONO = ("azeret-mono-latin.woff2", (("wght", 500),))


def glyphset(face):
    fname, loc = face
    return font(fname).getGlyphSet(location=dict(loc)), font(fname).getBestCmap(), font(fname)["head"].unitsPerEm


def advance(s, size, face):
    gs, cmap, upm = glyphset(face)
    return sum(gs[cmap[ord(ch)]].width for ch in s) * size / upm


def text(s, x, y, size, face, fill, anchor="start"):
    """Path for `s` with its baseline at y."""
    gs, cmap, upm = glyphset(face)
    k = size / upm
    names = []
    for ch in s:
        if ord(ch) not in cmap:
            sys.exit(f"{face[0]} has no glyph for {ch!r}")
        names.append(cmap[ord(ch)])
    width = sum(gs[n].width * k for n in names)
    pen_x = {"start": x, "middle": x - width / 2, "end": x - width}[anchor]
    uses = []
    for n in names:
        key = (face, n)
        if key not in _glyphs:
            pen = SVGPathPen(gs, ntos=lambda v: "%g" % round(v, 1))
            gs[n].draw(pen)
            _glyphs[key] = (f"g{len(_glyphs)}", pen.getCommands())
        gid, d = _glyphs[key]
        if d:
            uses.append(f'<use href="#{gid}" transform="matrix({k:.5f} 0 0 {-k:.5f} {pen_x:.2f} {y:.2f})"/>')
        pen_x += gs[n].width * k
    return f'<g fill="{fill}">{"".join(uses)}</g>'


def pct(t):
    return f"{t / CYCLE * 100:.3f}%"


def toggle(name, windows):
    """Keyframes (run with step-end) that show an element only inside the given (start, end) windows."""
    shown = lambda t: int(any(a <= t < b for a, b in windows))
    points = sorted({0.0} | {t for w in windows for t in w if 0 < t < CYCLE})
    body = "".join(f"{pct(t)}{{opacity:{shown(t)}}}" for t in points)
    return f"@keyframes {name}{{{body}100%{{opacity:{shown(0.0)}}}}}"


def ease_out(p):
    return 1 - (1 - p) ** 3


def keycap(label, x, y, c, lit):
    """A key cap with its top-left at (x, y); returns (svg, width)."""
    w = advance(label, 14, MONO) + 18
    stroke, fill, ink = (c["ember"], c["raised"], c["ink"]) if lit else (c["line"], c["plate"], c["mute"])
    return (f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="26" rx="6" fill="{fill}" stroke="{stroke}"/>'
            + text(label, x + w / 2, y + 18, 14, MONO, ink, anchor="middle"), w)


def build(theme):
    c = THEMES[theme]
    sx, sy, sw, sh = SCREEN
    (ax0, ay0), (ax1, ay1) = SEL0, SEL1
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img">']

    # --- when each part shows ---
    frames = [T_DRAG + (T_SEL - T_DRAG) * i / DRAG_FRAMES for i in range(DRAG_FRAMES)]
    win = {
        "dim0": [(T_DIM, T_DRAG)],
        "sel": [(T_SEL, T_CLOSE)],
        "bar": [(T_BAR, T_CLOSE)],
        "arrow": [(T_ARROW, T_CLOSE)], "head": [(T_ARROW_END, T_CLOSE)],
        "box": [(T_BOX, T_CLOSE)], "mark": [(T_MARK, T_CLOSE)],
        "toolA": [(T_BAR, T_R)], "toolR": [(T_R, T_N)], "toolN": [(T_N, T_CLOSE)],
        "toast": [(T_CLOSE, T_TOAST_END)],
        "k1": [(T_KEY, T_DRAG)], "k2": [(T_DRAG, T_SEL)], "k3": [(T_ARROW - 0.3, T_R)], "k4": [(T_R, T_N)],
        "k5": [(T_N, T_COPY)], "k6": [(T_COPY, T_TOAST_END)],
    }
    for i, t in enumerate(frames):
        win[f"f{i}"] = [(t, frames[i + 1] if i + 1 < DRAG_FRAMES else T_SEL)]
    # Without motion: the finished capture, before the copy.
    still = {"sel", "bar", "arrow", "head", "box", "mark", "toolN"}
    css = [toggle(k, v) for k, v in win.items()]
    rules = "".join(f".{k}{{animation:{k} {CYCLE}s step-end infinite}}" for k in win)
    base = "".join(f".{k}{{opacity:{1 if k in still else 0}}}" for k in win)

    # The arrow's shaft and the box draw themselves (stroke-dashoffset), and the cursor follows.
    alen = math.dist(ARROW[0], ARROW[1]) - 10
    bx, by, bw, bh = BOX
    blen = 2 * (bw + bh)
    css.append("@keyframes draw-arrow{0%," + pct(T_ARROW) + f"{{stroke-dashoffset:{alen:.1f}}}" + pct(T_ARROW_END) + ",100%{stroke-dashoffset:0}}")
    css.append("@keyframes draw-box{0%," + pct(T_BOX) + f"{{stroke-dashoffset:{blen}}}" + pct(T_BOX_END) + ",100%{stroke-dashoffset:0}}")
    rules += f".shaft{{animation:draw-arrow {CYCLE}s linear infinite}}.boxline{{animation:draw-box {CYCLE}s linear infinite}}"
    # Cursor: (time, x, y, visible); held between points, moved linearly where two points differ.
    path = [(0, ax0, ay0, 0), (T_DIM - 0.001, ax0, ay0, 0), (T_DIM, ax0, ay0, 1)]   # appears with the overlay
    for i, t in enumerate(frames):
        p = ease_out((i + 1) / DRAG_FRAMES)
        path.append((t, ax0 + (ax1 - ax0) * p, ay0 + (ay1 - ay0) * p, 1))
    path += [(T_SEL, ax1, ay1, 1), (T_ARROW - 0.25, *ARROW[0], 1), (T_ARROW, *ARROW[0], 1), (T_ARROW_END, *ARROW[1], 1),
             (T_BOX - 0.2, bx, by, 1), (T_BOX, bx, by, 1), (T_BOX_END, bx + bw, by + bh, 1),
             (T_MARK - 0.25, *MARK, 1), (T_MARK, *MARK, 1), (T_MARK + 0.3, MARK[0] + 26, MARK[1] + 26, 1),
             (T_CLOSE, MARK[0] + 26, MARK[1] + 26, 1), (T_CLOSE + 0.001, MARK[0] + 26, MARK[1] + 26, 0)]
    ck = "".join(f"{pct(t)}{{transform:translate({x:.1f}px,{y:.1f}px);opacity:{v}}}" for t, x, y, v in path)
    css.append(f"@keyframes cursor{{{ck}100%{{transform:translate({ax0}px,{ay0}px);opacity:0}}}}")
    rules += f".cursor{{animation:cursor {CYCLE}s linear infinite}}"
    base += ".cursor{opacity:0}.shaft{stroke-dashoffset:0}.boxline{stroke-dashoffset:0}"
    o.append(f"<style>{base}@media (prefers-reduced-motion:no-preference){{{''.join(css)}{rules}}}</style>")

    # --- the plate and the screen being captured ---
    o.append(f'<rect x="0.5" y="0.5" width="{W - 1}" height="{H - 1}" rx="22" fill="{c["plate"]}" stroke="{c["line"]}"/>')
    o.append(f'<clipPath id="scr"><rect x="{sx}" y="{sy}" width="{sw}" height="{sh}" rx="12"/></clipPath>')
    o.append('<g clip-path="url(#scr)">')
    o.append(f'<rect x="{sx}" y="{sy}" width="{sw}" height="{sh}" fill="{c["bg"]}"/>')
    # A window: title bar, a page of text on the left, a chart card on the right.
    o.append(f'<rect x="70" y="56" width="860" height="380" rx="10" fill="{c["plate"]}" stroke="{c["line"]}"/>')
    o.append(f'<rect x="70.5" y="56.5" width="859" height="33" rx="10" fill="{c["raised"]}"/>')
    for i in range(3):
        o.append(f'<circle cx="{92 + i * 18}" cy="73" r="5" fill="{c["strong"]}"/>')
    o.append(text("Weekly report", 500, 78, 14, SANS_R, c["mute"], anchor="middle"))
    o.append(f'<rect x="104" y="122" width="250" height="18" rx="4" fill="{c["ink"]}" fill-opacity=".85"/>')
    for i, wd in enumerate((330, 300, 344, 260, 318, 290, 336, 210)):
        o.append(f'<rect x="104" y="{162 + i * 26}" width="{wd}" height="10" rx="5" fill="{c["strong"]}"/>')
    o.append(f'<rect x="520" y="116" width="276" height="160" rx="8" fill="{c["bg"]}" stroke="{c["line"]}"/>')
    o.append(f'<rect x="536" y="130" width="96" height="10" rx="5" fill="{c["strong"]}"/>')
    for i, hgt in enumerate((40, 64, 52, 60, 112, 72, 58)):
        o.append(f'<rect x="{540 + i * 34}" y="{258 - hgt}" width="20" height="{hgt}" rx="3" fill="{c["mute"]}" fill-opacity="{1 if i == 4 else 0.55}"/>')
    o.append(f'<rect x="536" y="258" width="246" height="1.5" fill="{c["line"]}"/>')
    o.append(f'<rect x="104" y="388" width="150" height="30" rx="8" fill="{c["raised"]}" stroke="{c["line"]}"/>')
    o.append(f'<rect x="868" y="388" width="44" height="30" rx="8" fill="{c["raised"]}" stroke="{c["line"]}"/>')

    # --- the overlay: dim all, then a frame per step of the drag, then the settled selection ---
    dim = f'fill="{c["dim"]}" fill-opacity="{c["dim_a"]}"'

    def overlay(x0, y0, x1, y1, handles):
        g = [f'<rect x="{sx}" y="{sy}" width="{sw}" height="{y0 - sy}" {dim}/>',
             f'<rect x="{sx}" y="{y1}" width="{sw}" height="{sy + sh - y1}" {dim}/>',
             f'<rect x="{sx}" y="{y0}" width="{x0 - sx}" height="{y1 - y0}" {dim}/>',
             f'<rect x="{x1}" y="{y0}" width="{sx + sw - x1}" height="{y1 - y0}" {dim}/>',
             f'<rect x="{x0}" y="{y0}" width="{x1 - x0}" height="{y1 - y0}" fill="none" stroke="{c["ember"]}" stroke-width="2"/>']
        if handles:
            for hx in (x0, (x0 + x1) / 2, x1):
                for hy in (y0, (y0 + y1) / 2, y1):
                    if (hx, hy) != ((x0 + x1) / 2, (y0 + y1) / 2):
                        g.append(f'<rect x="{hx - 4}" y="{hy - 4}" width="8" height="8" rx="1.5" fill="{c["ember"]}"/>')
        label = f"{x1 - x0} × {y1 - y0}"
        lw = advance(label, 13, MONO) + 16
        g.append(f'<rect x="{x1 - lw:.1f}" y="{y1 + 8}" width="{lw:.1f}" height="22" rx="5" fill="{c["plate"]}" stroke="{c["line"]}"/>')
        g.append(text(label, x1 - lw / 2, y1 + 23.5, 13, MONO, c["ink"], anchor="middle"))
        return "".join(g)

    o.append(f'<g class="dim0"><rect x="{sx}" y="{sy}" width="{sw}" height="{sh}" {dim}/></g>')
    for i in range(DRAG_FRAMES):
        p = ease_out((i + 1) / DRAG_FRAMES)
        x1, y1 = round(ax0 + (ax1 - ax0) * p), round(ay0 + (ay1 - ay0) * p)
        o.append(f'<g class="f{i}">{overlay(ax0, ay0, max(x1, ax0 + 2), max(y1, ay0 + 2), False)}</g>')
    o.append(f'<g class="sel">{overlay(ax0, ay0, ax1, ay1, True)}</g>')

    # --- the marks: arrow (blue), box (red), step marker 1 (yellow) ---
    (tx, ty), (hx, hy) = ARROW
    ang = math.atan2(hy - ty, hx - tx)
    ex, ey = hx - 10 * math.cos(ang), hy - 10 * math.sin(ang)
    o.append(f'<g class="arrow"><line class="shaft" x1="{tx}" y1="{ty}" x2="{ex:.1f}" y2="{ey:.1f}" stroke="{PRESETS[BLUE]}" stroke-width="5" '
             f'stroke-linecap="round" stroke-dasharray="{alen:.1f}"/></g>')
    pts = [(hx, hy)] + [(hx - 20 * math.cos(ang + s), hy - 20 * math.sin(ang + s)) for s in (0.45, -0.45)]
    o.append(f'<g class="head"><polygon points="{" ".join(f"{x:.1f},{y:.1f}" for x, y in pts)}" fill="{PRESETS[BLUE]}" stroke="{PRESETS[BLUE]}" stroke-width="2" stroke-linejoin="round"/></g>')
    o.append(f'<g class="box"><path class="boxline" d="M{bx} {by}h{bw}v{bh}h{-bw}z" fill="none" stroke="{PRESETS[RED]}" stroke-width="4" '
             f'stroke-linejoin="round" stroke-dasharray="{blen}"/></g>')
    mx, my = MARK
    o.append(f'<g class="mark"><circle cx="{mx}" cy="{my}" r="14" fill="{PRESETS[YELLOW]}"/>{text("1", mx, my + 5.5, 16, SANS, "#1A1B1E", anchor="middle")}</g>')

    # --- the toolbar under the selection: tool keys, then the five presets ---
    tb_w = len(TOOLS) * 30 + 22 + len(PRESETS) * 26 + 16
    tb_x, tb_y = (ax0 + ax1) / 2 - tb_w / 2, ay1 + 40
    g = [f'<rect x="{tb_x:.1f}" y="{tb_y}" width="{tb_w}" height="40" rx="10" fill="{c["plate"]}" stroke="{c["line"]}"/>']
    for i, ch in enumerate(TOOLS):
        g.append(text(ch, tb_x + 23 + i * 30, tb_y + 25, 14, MONO, c["mute"], anchor="middle"))
    sep = tb_x + 8 + len(TOOLS) * 30 + 11
    g.append(f'<rect x="{sep:.1f}" y="{tb_y + 9}" width="1" height="22" fill="{c["line"]}"/>')
    sw0 = sep + 12
    for i, col in enumerate(PRESETS):
        g.append(f'<rect x="{sw0 + i * 26:.1f}" y="{tb_y + 10}" width="20" height="20" rx="5" fill="{col}"/>')
    # The tool and the preset in use: A and blue, R and red, N and yellow.
    for cls, ch, preset in (("toolA", "A", BLUE), ("toolR", "R", RED), ("toolN", "N", YELLOW)):
        i = TOOLS.index(ch)
        cx = tb_x + 23 + i * 30
        g.append(f'<g class="{cls}"><rect x="{cx - 12:.1f}" y="{tb_y + 7}" width="24" height="26" rx="6" fill="{c["ember"]}"/>'
                 + text(ch, cx, tb_y + 25, 14, MONO, c["on_ember"], anchor="middle")
                 + f'<rect x="{sw0 + preset * 26 - 3:.1f}" y="{tb_y + 7}" width="26" height="26" rx="7" fill="none" stroke="{c["ink"]}" stroke-width="2"/></g>')
    o.append(f'<g class="bar">{"".join(g)}</g>')

    # --- the crosshair cursor ---
    cross = "".join(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{c["ember"]}"/>'
                    for x, y, w, h in ((-11, -1, 7, 2), (4, -1, 7, 2), (-1, -11, 2, 7), (-1, 4, 2, 7)))
    o.append(f'<g class="cursor"><g stroke="{c["bg"]}" stroke-width="2" paint-order="stroke">{cross}</g></g>')

    # --- after Ctrl+C: the overlay is gone, and the app's popup says so ---
    px, py, pw, ph = 664, 320, 268, 84
    o.append(f'<g class="toast"><rect x="{px}" y="{py}" width="{pw}" height="{ph}" rx="12" fill="{c["plate"]}" stroke="{c["line"]}"/>'
             f'<circle cx="{px + 30}" cy="{py + 30}" r="12" fill="{c["mint"]}"/>'
             f'<path d="M{px + 24} {py + 30.5}l4 4l8 -8" fill="none" stroke="{c["plate"]}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>'
             + text("Copied", px + 54, py + 36, 17, SANS, c["ink"])
             + text(f"{ax1 - ax0} × {ay1 - ay0}", px + 54 + advance("Copied", 17, SANS) + 12, py + 36, 14, MONO, c["mute"])
             + text("Paste it anywhere with Ctrl+V", px + 54, py + 62, 14, SANS_R, c["mute"]) + "</g>")
    o.append("</g>")   # end of the screen

    # --- the strip: each step's keys, lit while it happens ---
    steps = [("k1", ["Ctrl", "Shift", "S"], "capture"), ("k2", [], "drag a region"), ("k3", ["A"], "arrow"),
             ("k4", ["R"], "box"), ("k5", ["N"], "step"), ("k6", ["Ctrl", "C"], "copy")]
    x, y = 40, 450
    for cls, keys, label in steps:
        for lit in (False, True):
            g, kx = [], x
            for k in keys:
                svg, w = keycap(k, kx, y, c, lit)
                g.append(svg)
                kx += w + 4
            g.append(text(label, kx + (4 if keys else 0), y + 18, 15, SANS_R, c["ink"] if lit else c["mute"]))
            o.append(f'<g class="{cls}">{"".join(g)}</g>' if lit else "".join(g))
        x = kx + (4 if keys else 0) + advance(label, 15, SANS_R) + 30

    defs = "".join(f'<path id="{gid}" d="{d}"/>' for gid, d in _glyphs.values() if d)
    o.insert(1, f"<defs>{defs}</defs>")
    o.append("</svg>")
    return "".join(o)


for theme in THEMES:
    _glyphs.clear()
    path = os.path.join(OUT, f"panel-{theme}.svg")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(build(theme) + "\n")
    print(f"wrote {os.path.relpath(path)} ({os.path.getsize(path) // 1024} KB)")
