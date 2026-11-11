use screen_mark::config::Config;
use screen_mark::geometry::Color;

#[test]
fn default_config_has_correct_values() {
    let cfg = Config::default();
    assert_eq!(cfg.general.default_color, "red");
    assert_eq!(cfg.general.default_thickness, 3.0);
    assert_eq!(cfg.general.save_directory, "");
    assert_eq!(cfg.hotkey.region, "Ctrl+Shift+S");
}

#[test]
fn parse_color_valid() {
    let mut cfg = Config::default();
    cfg.general.default_color = "blue".to_string();
    assert_eq!(cfg.default_color(), Color::blue());

    cfg.general.default_color = "green".to_string();
    assert_eq!(cfg.default_color(), Color::green());

    cfg.general.default_color = "yellow".to_string();
    assert_eq!(cfg.default_color(), Color::yellow());

    cfg.general.default_color = "white".to_string();
    assert_eq!(cfg.default_color(), Color::white());

    cfg.general.default_color = "red".to_string();
    assert_eq!(cfg.default_color(), Color::red());
}

#[test]
fn parse_color_invalid_falls_back_to_red() {
    let mut cfg = Config::default();
    cfg.general.default_color = "magenta".to_string();
    assert_eq!(cfg.default_color(), Color::red());

    cfg.general.default_color = "".to_string();
    assert_eq!(cfg.default_color(), Color::red());
}

#[test]
fn serialize_roundtrip_via_toml() {
    let cfg = Config::default();
    let serialized = toml::to_string(&cfg).expect("serialize");
    let deserialized: Config = toml::from_str(&serialized).expect("deserialize");
    assert_eq!(
        deserialized.general.default_color,
        cfg.general.default_color
    );
    assert_eq!(
        deserialized.general.default_thickness,
        cfg.general.default_thickness
    );
    assert_eq!(
        deserialized.general.save_directory,
        cfg.general.save_directory
    );
    assert_eq!(deserialized.hotkey.region, cfg.hotkey.region);
}

#[test]
fn partial_config_parses_with_defaults() {
    // A config missing keys/sections (older version, hand-edited) must parse
    // instead of silently resetting everything to defaults.
    let toml_src = r#"
[general]
default_color = "blue"

[shortcuts]
arrow = "q"
"#;
    let cfg: Config = toml::from_str(toml_src).expect("partial config should parse");
    assert_eq!(cfg.general.default_color, "blue");
    assert_eq!(cfg.general.default_thickness, 3.0); // default filled in
    assert!(cfg.general.history_enabled); // default filled in
    assert_eq!(cfg.hotkey.region, "Ctrl+Shift+S"); // whole section defaulted
    assert_eq!(cfg.shortcuts.arrow, "q");
    assert_eq!(cfg.shortcuts.rectangle, "r"); // sibling key defaulted
    assert!(cfg.toolbar.select);
}

#[test]
fn empty_config_parses_as_defaults() {
    let cfg: Config = toml::from_str("").expect("empty config should parse");
    assert_eq!(cfg.general.default_color, "red");
    assert_eq!(cfg.hotkey.region, "Ctrl+Shift+S");
}

#[test]
fn parse_color_hex() {
    let mut cfg = Config::default();
    cfg.general.default_color = "#5cb8ff".to_string();
    let c = cfg.default_color();
    assert!((c.r - 0x5c as f32 / 255.0).abs() < 0.001);
    assert!((c.g - 0xb8 as f32 / 255.0).abs() < 0.001);
    assert!((c.b - 0xff as f32 / 255.0).abs() < 0.001);

    // Malformed hex falls back to red
    cfg.general.default_color = "#zzzzzz".to_string();
    assert_eq!(cfg.default_color(), Color::red());
    cfg.general.default_color = "#fff".to_string();
    assert_eq!(cfg.default_color(), Color::red());
}

#[test]
fn thickness_clamping() {
    let mut cfg = Config::default();

    cfg.general.default_thickness = 100.0;
    assert_eq!(cfg.clamped_thickness(), 20.0);

    cfg.general.default_thickness = -5.0;
    assert_eq!(cfg.clamped_thickness(), 1.0);

    cfg.general.default_thickness = 10.0;
    assert_eq!(cfg.clamped_thickness(), 10.0);
}

#[test]
fn theme_defaults_to_dark() {
    let cfg = screen_mark::config::Config::default();
    assert_eq!(cfg.general.theme, "dark");
    assert_eq!(cfg.theme_mode(), screen_mark::theme::ThemeMode::Dark);
}

#[test]
fn theme_mode_parses_light_case_insensitively() {
    let mut cfg = screen_mark::config::Config::default();
    cfg.general.theme = "Light".to_string();
    assert_eq!(cfg.theme_mode(), screen_mark::theme::ThemeMode::Light);
    cfg.general.theme = "garbage".to_string();
    assert_eq!(cfg.theme_mode(), screen_mark::theme::ThemeMode::Dark);
}

#[test]
fn updates_are_off_and_from_github_unless_set() {
    let cfg = Config::default();
    assert!(!cfg.updates.auto_check);
    assert_eq!(cfg.updates.url, "");
    // A config written before there was an [updates] section.
    let old: Config = toml::from_str("[general]\ntheme = \"light\"\n").unwrap();
    assert!(!old.updates.auto_check);
    assert_eq!(old.updates.url, "");

    let set: Config = toml::from_str(
        "[updates]\nauto_check = true\nurl = \"https://intranet.example/screen-mark/downloads\"\n",
    )
    .unwrap();
    assert!(set.updates.auto_check);
    let again: Config = toml::from_str(&toml::to_string(&set).unwrap()).unwrap();
    assert!(again.updates.auto_check);
    assert_eq!(again.updates.url, set.updates.url);

    // The proxy follows the system unless it is set.
    assert_eq!(cfg.updates.proxy, "");
    assert_eq!(old.updates.proxy, "");
    let proxied: Config =
        toml::from_str("[updates]\nproxy = \"http://proxy.corp:8080\"\n").unwrap();
    assert_eq!(proxied.updates.proxy, "http://proxy.corp:8080");
    let again: Config = toml::from_str(&toml::to_string(&proxied).unwrap()).unwrap();
    assert_eq!(again.updates.proxy, proxied.updates.proxy);
}
