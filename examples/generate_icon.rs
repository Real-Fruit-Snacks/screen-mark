//! Generate the screen-mark Cyclorama Dawn v2.1.0 icon (mitered capture brackets +
//! Beacon Orange arrow + rising Steel -> Beacon horizon wash & rule) using resvg.
//! Run: cargo run --example generate_icon
//!
//! Palette: Real-Fruit-Snacks/rfs-brand.

fn emblem_svg(small: bool) -> String {
    let wash_op = if small { "0.86" } else { "0.74" };
    let frame_stroke = if small {
        "#3B3E4A"
    } else {
        "rgba(255,255,255,0.16)"
    };
    let bw = if small { 3.6 } else { 2.6 };
    let aw = if small { 4.4 } else { 3.4 };
    let arrow_col = if small { "#FF9254" } else { "#F06E2B" };
    let bar_h = if small { 4.2 } else { 3.6 };
    let bar_y = if small { 50.2 } else { 50.5 };
    let bar_r = bar_h / 2.0;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <defs>
    <linearGradient id="cyc-h" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#133954"/>
      <stop offset="44%" stop-color="#1F547A"/>
      <stop offset="76%" stop-color="#CC5016"/>
      <stop offset="100%" stop-color="#F06E2B"/>
    </linearGradient>
    <linearGradient id="cyc-f" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0"/>
      <stop offset="42%" stop-color="#FFFFFF" stop-opacity="0.08"/>
      <stop offset="74%" stop-color="#FFFFFF" stop-opacity="0.58"/>
      <stop offset="100%" stop-color="#FFFFFF" stop-opacity="1"/>
    </linearGradient>
    <mask id="cyc-m" maskContentUnits="objectBoundingBox">
      <rect width="1" height="1" fill="url(#cyc-f)"/>
    </mask>
    <linearGradient id="rule-d" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#74B8E8"/>
      <stop offset="52%" stop-color="#3B85B8"/>
      <stop offset="100%" stop-color="#F06E2B"/>
    </linearGradient>
  </defs>
  <rect width="64" height="64" rx="14" fill="#050507"/>
  <rect width="64" height="64" rx="14" fill="url(#cyc-h)" opacity="{wash_op}" mask="url(#cyc-m)"/>
  <rect x="0.75" y="0.75" width="62.5" height="62.5" rx="13.25" fill="none" stroke="{frame_stroke}" stroke-width="1.5"/>
  <g fill="none" stroke="#F4F5F8" stroke-width="{bw}" stroke-linecap="square" stroke-linejoin="miter">
    <path d="M12.5 19.5 V11.5 H20.5"/>
    <path d="M43.5 11.5 H51.5 V19.5"/>
    <path d="M12.5 35.5 V43.5 H20.5"/>
    <path d="M43.5 43.5 H51.5 V35.5"/>
  </g>
  <line x1="21.5" y1="36.2" x2="36.2" y2="21.5" stroke="{arrow_col}" stroke-width="{aw}" stroke-linecap="square"/>
  <polygon points="41.8,15.9 30.2,18.4 39.3,27.5" fill="{arrow_col}"/>
  <rect x="11.0" y="{bar_y}" width="42.0" height="{bar_h}" rx="{bar_r:.2}" fill="url(#rule-d)"/>
</svg>"##
    )
}

fn render_png(size: u32, path: &str) {
    let svg = emblem_svg(size <= 32);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
        .expect("valid emblem SVG");
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("non-zero icon size");
    let scale = size as f32 / 64.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(path).expect("save PNG");
}

fn main() {
    render_png(256, "assets/icon.png");
    render_png(256, "docs/icon_256.png");
    println!("Icons generated.");
}
