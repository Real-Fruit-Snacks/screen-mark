//! Generate the screen-mark icon (capture brackets + an Ember arrow) using tiny-skia.
//! Run: cargo run --example generate_icon
//!
//! RFS Nightshift badge: a graphite tile with selection brackets in the text
//! color framing an Ember arrow. Palette: Real-Fruit-Snacks/.github/BRAND.md.

fn main() {
    generate_icon(256, "icon_256.png");
    generate_icon(256, "assets/icon.png");
    generate_icon(48, "icon_48.png");
    generate_icon(16, "icon_16.png");
    println!("Icons generated.");
}

fn generate_icon(size: u32, path: &str) {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
    let s = size as f32 / 128.0; // design is 128x128

    // ─── 1. Rounded square background ───
    let corner_r = 26.0 * s;
    if let Some(bg_path) = rounded_rect(0.0, 0.0, size as f32, size as f32, corner_r) {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba(0.047, 0.051, 0.063, 1.0).unwrap()); // #0c0d10 (bg)
        paint.anti_alias = true;
        pixmap.fill_path(
            &bg_path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // ─── 2. Subtle inner glow / vignette ───
    // A slightly lighter rectangle inset to give depth
    let inset = 3.0 * s;
    if let Some(inner_path) = rounded_rect(
        inset,
        inset,
        size as f32 - inset * 2.0,
        size as f32 - inset * 2.0,
        corner_r - inset,
    ) {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba(0.075, 0.082, 0.102, 1.0).unwrap()); // #13151a (surface)
        paint.anti_alias = true;
        pixmap.fill_path(
            &inner_path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // ─── 3. Selection brackets (corners) ───
    let bracket_inset = 18.0 * s;
    let bracket_len = 22.0 * s;
    // Scale bracket width for small sizes
    let bracket_w = if size <= 32 { 5.0 * s } else { 3.5 * s };

    let bracket_color = tiny_skia::Color::from_rgba(0.910, 0.902, 0.882, 1.0).unwrap(); // #e8e6e1 (text)
    let mut bp = tiny_skia::Paint::default();
    bp.set_color(bracket_color);
    bp.anti_alias = true;
    let bs = tiny_skia::Stroke {
        width: bracket_w,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..tiny_skia::Stroke::default()
    };

    let x1 = bracket_inset;
    let y1 = bracket_inset;
    let x2 = size as f32 - bracket_inset;
    let y2 = size as f32 - bracket_inset;

    // Top-left
    draw_bracket(&mut pixmap, x1, y1, bracket_len, true, true, &bp, &bs);
    // Top-right
    draw_bracket(&mut pixmap, x2, y1, bracket_len, false, true, &bp, &bs);
    // Bottom-left
    draw_bracket(&mut pixmap, x1, y2, bracket_len, true, false, &bp, &bs);
    // Bottom-right
    draw_bracket(&mut pixmap, x2, y2, bracket_len, false, false, &bp, &bs);

    // ─── 4. The mark: an Ember arrow, the tool's signature annotation ───
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let ember = tiny_skia::Color::from_rgba(1.0, 0.416, 0.239, 1.0).unwrap(); // #ff6a3d
    let shaft_w = if size <= 32 { 7.0 * s } else { 6.0 * s };
    // Tail at lower-left, head at upper-right; the head is a filled triangle.
    let (tx, ty) = (cx - 22.0 * s, cy + 22.0 * s);
    let (hx, hy) = (cx + 24.0 * s, cy - 24.0 * s);
    let head_len = 22.0 * s;
    let head_half = 12.0 * s;
    // Shaft stops short of the tip so the triangle owns the point.
    let (dx, dy) = (hx - tx, hy - ty);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);
    let (sx2, sy2) = (hx - ux * head_len * 0.75, hy - uy * head_len * 0.75);

    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(tx, ty);
    pb.line_to(sx2, sy2);
    if let Some(shaft) = pb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(ember);
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: shaft_w,
            line_cap: tiny_skia::LineCap::Round,
            ..tiny_skia::Stroke::default()
        };
        pixmap.stroke_path(
            &shaft,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let (bx, by) = (hx - ux * head_len, hy - uy * head_len);
    let (px, py) = (-uy, ux);
    let mut hb = tiny_skia::PathBuilder::new();
    hb.move_to(hx, hy);
    hb.line_to(bx + px * head_half, by + py * head_half);
    hb.line_to(bx - px * head_half, by - py * head_half);
    hb.close();
    if let Some(head) = hb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(ember);
        paint.anti_alias = true;
        pixmap.fill_path(
            &head,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // ─── 5. Outer badge border (very subtle) ───
    if size >= 48 {
        if let Some(badge_path) =
            rounded_rect(0.5, 0.5, size as f32 - 1.0, size as f32 - 1.0, corner_r)
        {
            let mut border = tiny_skia::Paint::default();
            border.set_color(tiny_skia::Color::from_rgba(0.149, 0.165, 0.200, 1.0).unwrap()); // #262a33 (line)
            border.anti_alias = true;
            let bord_stroke = tiny_skia::Stroke {
                width: 1.0 * s,
                ..tiny_skia::Stroke::default()
            };
            pixmap.stroke_path(
                &badge_path,
                &border,
                &bord_stroke,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }

    pixmap.save_png(path).unwrap();
}

// ─── Helper: rounded rectangle path ───
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

// ─── Helper: L-shaped bracket ───
#[allow(clippy::too_many_arguments)]
fn draw_bracket(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    len: f32,
    left: bool,
    top: bool,
    paint: &tiny_skia::Paint,
    stroke: &tiny_skia::Stroke,
) {
    let dx = if left { len } else { -len };
    let dy = if top { len } else { -len };
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + dx, y);
    pb.line_to(x, y);
    pb.line_to(x, y + dy);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, paint, stroke, tiny_skia::Transform::identity(), None);
    }
}

// ─── Helper: full droplet path ───
