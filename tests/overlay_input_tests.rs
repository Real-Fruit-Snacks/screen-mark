//! The capture overlay, driven like a user would: move the mouse, press and
//! release it, press keys. No window or display needed.

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::geometry::{Color, Point};
use screen_mark::keys::{KeyMap, Mods, Named};
use screen_mark::overlay::input::{KeyInput, Outcome};
use screen_mark::overlay::selection::Selection;
use screen_mark::state::OverlayState;
use screen_mark::tools::{Annotation, ToolKind};
use winit::window::CursorIcon;

const W: u32 = 800;
const H: u32 = 600;

/// A capture whose pixels are (x, y, 0), so a colour says where it is from.
fn capture() -> CapturedScreen {
    let mut pixels = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            pixels[i] = (x & 0xff) as u8;
            pixels[i + 1] = (y & 0xff) as u8;
            pixels[i + 3] = 255;
        }
    }
    CapturedScreen {
        pixels,
        width: W,
        height: H,
        x_offset: 0,
        y_offset: 0,
        scale_factor: 1.0,
    }
}

fn overlay() -> OverlayState {
    OverlayState::new(capture(), &Config::default())
}

fn keymap() -> KeyMap {
    KeyMap::from_report(&Config::default().key_report())
}

fn drag(o: &mut OverlayState, from: (f32, f32), to: (f32, f32)) -> Outcome {
    o.on_cursor_moved(Point::new(from.0, from.1));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(to.0, to.1));
    o.on_mouse_up()
}

fn click(o: &mut OverlayState, at: (f32, f32)) -> Outcome {
    o.on_cursor_moved(Point::new(at.0, at.1));
    let down = o.on_mouse_down();
    let up = o.on_mouse_up();
    if matches!(down, Outcome::Nothing | Outcome::Redraw) {
        up
    } else {
        down
    }
}

/// An overlay with a 400 x 300 region selected at (100, 100).
fn with_region() -> OverlayState {
    let mut o = overlay();
    drag(&mut o, (100.0, 100.0), (500.0, 400.0));
    assert_eq!(
        o.selection,
        Some(Selection {
            x: 100.0,
            y: 100.0,
            width: 400.0,
            height: 300.0
        })
    );
    o
}

fn key(o: &mut OverlayState, input: KeyInput) -> Outcome {
    o.on_key(&input, &keymap())
}

fn type_text(o: &mut OverlayState, s: &str) {
    for c in s.chars() {
        key(o, KeyInput::typed(&c.to_string()));
    }
}

/// Click the toolbar button for `kind`.
fn pick_tool(o: &mut OverlayState, kind: ToolKind) {
    use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
    let button = BUTTONS
        .iter()
        .position(|b| b.action == ButtonAction::Tool(kind))
        .expect("tool has a button");
    let visible = o
        .visible_buttons
        .iter()
        .position(|b| *b == button)
        .expect("button is shown");
    let (x, y, w, h) = o.toolbar().expect("toolbar").button_rect(visible);
    o.on_cursor_moved(Point::new(x + w / 2.0, y + h / 2.0));
    assert_eq!(o.on_mouse_down(), Outcome::Redraw);
    o.on_mouse_up();
    assert_eq!(o.active_tool, kind);
}

// ───────────── selecting a region ─────────────

#[test]
fn dragging_selects_a_region() {
    let mut o = overlay();
    assert_eq!(
        drag(&mut o, (100.0, 100.0), (500.0, 400.0)),
        Outcome::Redraw
    );
    assert!(!o.is_selecting);
    assert_eq!(
        o.selection.map(|s| (s.width, s.height)),
        Some((400.0, 300.0))
    );
}

#[test]
fn a_click_without_a_drag_selects_nothing() {
    let mut o = overlay();
    click(&mut o, (300.0, 300.0));
    assert_eq!(o.selection, None);
    // A drag of a pixel or two is a click with a shaky hand, not a region.
    drag(&mut o, (300.0, 300.0), (302.0, 301.0));
    assert_eq!(o.selection, None);
}

#[test]
fn finish_on_release_copies_as_soon_as_the_region_is_drawn() {
    let mut config = Config::default();
    config.general.finish_on_release = true;
    let mut o = OverlayState::new(capture(), &config);
    assert_eq!(drag(&mut o, (100.0, 100.0), (500.0, 400.0)), Outcome::Copy);
    // A stray click still copies nothing.
    let mut o = OverlayState::new(capture(), &config);
    assert_eq!(click(&mut o, (100.0, 100.0)), Outcome::Redraw);
}

#[test]
fn a_press_outside_the_region_starts_a_new_one() {
    let mut o = with_region();
    drag(&mut o, (600.0, 50.0), (700.0, 90.0));
    assert_eq!(
        o.selection,
        Some(Selection {
            x: 600.0,
            y: 50.0,
            width: 100.0,
            height: 40.0
        })
    );
}

#[test]
fn dragging_an_edge_resizes_the_region() {
    let mut o = with_region();
    // The right edge is at x = 500.
    drag(&mut o, (500.0, 250.0), (560.0, 250.0));
    assert_eq!(o.selection.map(|s| (s.x, s.width)), Some((100.0, 460.0)));
    assert_eq!(o.drag_zone, None);
}

#[test]
fn select_all_takes_the_whole_capture() {
    let mut o = overlay();
    let out = key(&mut o, KeyInput::typed("a").with(Mods::CTRL));
    assert_eq!(out, Outcome::Redraw);
    assert_eq!(
        o.selection.map(|s| (s.x, s.y, s.width, s.height)),
        Some((0.0, 0.0, W as f32, H as f32))
    );
}

// ───────────── fixed keys and the key map ─────────────

#[test]
fn escape_closes_and_enter_copies() {
    let mut o = overlay();
    // Nothing selected: Enter has nothing to copy.
    assert_eq!(key(&mut o, KeyInput::named(Named::Enter)), Outcome::Nothing);
    assert_eq!(key(&mut o, KeyInput::named(Named::Escape)), Outcome::Close);
    let mut o = with_region();
    assert_eq!(key(&mut o, KeyInput::named(Named::Enter)), Outcome::Copy);
    assert_eq!(key(&mut o, KeyInput::named(Named::Escape)), Outcome::Close);
}

#[test]
fn bound_keys_run_their_actions() {
    let mut o = with_region();
    assert_eq!(
        key(&mut o, KeyInput::typed("c").with(Mods::CTRL)),
        Outcome::Copy
    );
    assert_eq!(
        key(&mut o, KeyInput::typed("s").with(Mods::CTRL)),
        Outcome::Save
    );
    // Pin, OCR and upload are unbound by default.
    assert_eq!(
        key(&mut o, KeyInput::typed("p").with(Mods::CTRL)),
        Outcome::Nothing
    );
}

#[test]
fn a_bound_ctrl_enter_wins_over_the_fixed_enter() {
    let mut config = Config::default();
    config.keys.save = "Ctrl+Enter".into();
    let map = KeyMap::from_report(&config.key_report());
    let mut o = with_region();
    let press = KeyInput::named(Named::Enter).with(Mods::CTRL);
    assert_eq!(o.on_key(&press, &map), Outcome::Save);
}

#[test]
fn tool_keys_pick_tools_and_ignore_shift() {
    let mut o = with_region();
    assert_eq!(key(&mut o, KeyInput::typed("r")), Outcome::Redraw);
    assert_eq!(o.active_tool, ToolKind::Rectangle);
    // Caps Lock or a held Shift types "L"; it still picks the line tool.
    key(&mut o, KeyInput::typed("L").with(Mods::SHIFT));
    assert_eq!(o.active_tool, ToolKind::Line);
}

#[test]
fn a_2_0_tool_key_bound_to_any_typed_character_still_works() {
    let mut config = Config::default();
    config.shortcuts.pencil = "#".into();
    let map = KeyMap::from_report(&config.key_report());
    let mut o = with_region();
    assert_eq!(o.on_key(&KeyInput::typed("#"), &map), Outcome::Redraw);
    assert_eq!(o.active_tool, ToolKind::Pencil);
}

// ───────────── drawing, selecting and undoing annotations ─────────────

#[test]
fn every_drawing_tool_adds_one_annotation_per_drag() {
    use screen_mark::tools::TOOL_ORDER;
    for kind in TOOL_ORDER {
        if matches!(
            kind,
            ToolKind::Select | ToolKind::Eyedropper | ToolKind::Text
        ) {
            continue;
        }
        let mut o = with_region();
        o.active_tool = kind;
        let out = drag(&mut o, (150.0, 150.0), (300.0, 260.0));
        assert_eq!(out, Outcome::Redraw, "{kind:?}");
        assert_eq!(o.annotations.len(), 1, "{kind:?} should add an annotation");
        assert_eq!(o.undo_stack.len(), 1, "{kind:?} should be one undo step");
        // While the button is down the tool shows a preview (step markers
        // are placed on release and have none).
        o.on_cursor_moved(Point::new(150.0, 150.0));
        o.on_mouse_down();
        o.on_cursor_moved(Point::new(220.0, 240.0));
        let preview = o.tool(kind).and_then(|t| t.in_progress_annotation());
        assert_eq!(
            preview.is_some(),
            kind != ToolKind::StepMarker,
            "{kind:?} preview"
        );
    }
}

#[test]
fn undo_and_redo_walk_the_annotation_list() {
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    drag(&mut o, (150.0, 150.0), (250.0, 220.0));
    drag(&mut o, (300.0, 150.0), (400.0, 220.0));
    assert_eq!(o.annotations.len(), 2);

    let undo = KeyInput::typed("z").with(Mods::CTRL);
    let redo = KeyInput::typed("z").with(Mods::CTRL | Mods::SHIFT);
    assert_eq!(key(&mut o, undo.clone()), Outcome::Redraw);
    assert_eq!(o.annotations.len(), 1);
    assert_eq!(key(&mut o, redo), Outcome::Redraw);
    assert_eq!(o.annotations.len(), 2);
    key(&mut o, undo.clone());
    key(&mut o, undo.clone());
    assert!(o.annotations.is_empty());
    // Nothing left to undo: nothing to draw again.
    assert_eq!(key(&mut o, undo), Outcome::Nothing);
}

#[test]
fn the_select_tool_moves_an_annotation_as_one_undo_step() {
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    drag(&mut o, (150.0, 150.0), (250.0, 220.0));
    o.active_tool = ToolKind::Select;

    // Grab the rectangle by its left edge and move it 40 right, 10 down.
    o.on_cursor_moved(Point::new(150.0, 180.0));
    o.on_mouse_down();
    assert_eq!(o.selected_index, Some(0));
    o.on_cursor_moved(Point::new(170.0, 185.0));
    o.on_cursor_moved(Point::new(190.0, 190.0));
    o.on_mouse_up();
    let Annotation::Rectangle { top_left, .. } = &o.annotations[0] else {
        panic!("still a rectangle");
    };
    assert_eq!((top_left.x, top_left.y), (190.0, 160.0));
    assert_eq!(o.undo_stack.len(), 2, "add + one move");

    // Escape drops the selection first; a second Escape closes.
    assert_eq!(key(&mut o, KeyInput::named(Named::Escape)), Outcome::Redraw);
    assert_eq!(o.selected_index, None);
    assert_eq!(key(&mut o, KeyInput::named(Named::Escape)), Outcome::Close);
}

#[test]
fn delete_removes_the_selected_annotation_and_renumbers_steps() {
    let mut o = with_region();
    o.active_tool = ToolKind::StepMarker;
    click(&mut o, (150.0, 150.0));
    click(&mut o, (250.0, 150.0));
    assert_eq!(o.annotations.len(), 2);

    o.active_tool = ToolKind::Select;
    click(&mut o, (250.0, 150.0));
    assert_eq!(o.selected_index, Some(1));
    assert_eq!(key(&mut o, KeyInput::named(Named::Delete)), Outcome::Redraw);
    assert_eq!(o.annotations.len(), 1);

    // The next marker is 2 again, not 3.
    o.active_tool = ToolKind::StepMarker;
    click(&mut o, (350.0, 150.0));
    assert!(matches!(
        o.annotations[1],
        Annotation::StepMarker { number: 2, .. }
    ));
}

#[test]
fn a_swatch_recolours_the_selected_annotation_or_sets_the_drawing_colour() {
    use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
    let mut o = with_region();
    let swatch = |o: &OverlayState, preset: usize| {
        let button = BUTTONS
            .iter()
            .position(|b| b.action == ButtonAction::Color(preset))
            .unwrap();
        let visible = o.visible_buttons.iter().position(|b| *b == button).unwrap();
        let (x, y, w, h) = o.toolbar().unwrap().button_rect(visible);
        (x + w / 2.0, y + h / 2.0)
    };
    let presets = Color::presets();

    // Nothing selected: the swatch sets the colour new annotations get.
    let at = swatch(&o, 1);
    click(&mut o, at);
    assert_eq!(o.current_color, presets[1]);

    o.active_tool = ToolKind::Rectangle;
    drag(&mut o, (150.0, 150.0), (250.0, 220.0));
    o.active_tool = ToolKind::Select;
    click(&mut o, (150.0, 180.0));
    assert_eq!(o.selected_index, Some(0));

    // With the rectangle selected the swatch recolours it (one undo step)
    // and leaves the drawing colour alone.
    let at = swatch(&o, 2);
    click(&mut o, at);
    assert!(matches!(o.annotations[0], Annotation::Rectangle { color, .. } if color == presets[2]));
    assert_eq!(o.current_color, presets[1]);
    key(&mut o, KeyInput::typed("z").with(Mods::CTRL));
    assert!(matches!(o.annotations[0], Annotation::Rectangle { color, .. } if color == presets[1]));
}

#[test]
fn toolbar_buttons_report_their_actions() {
    use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
    let expect = [
        (ButtonAction::Copy, Outcome::Copy),
        (ButtonAction::Save, Outcome::Save),
        (ButtonAction::Pin, Outcome::Pin),
        (ButtonAction::Ocr, Outcome::Ocr),
        (ButtonAction::Upload, Outcome::Upload),
    ];
    for (action, outcome) in expect {
        let mut o = with_region();
        let button = BUTTONS.iter().position(|b| b.action == action).unwrap();
        let visible = o.visible_buttons.iter().position(|b| *b == button).unwrap();
        let (x, y, w, h) = o.toolbar().unwrap().button_rect(visible);
        o.on_cursor_moved(Point::new(x + w / 2.0, y + h / 2.0));
        assert_eq!(o.cursor_icon(), CursorIcon::Pointer);
        assert_eq!(o.on_mouse_down(), outcome, "{action:?}");
    }
}

#[test]
fn the_eyedropper_picks_the_colour_under_the_pointer() {
    let mut o = with_region();
    pick_tool(&mut o, ToolKind::Eyedropper);
    o.on_cursor_moved(Point::new(200.0, 150.0));
    let picked = o.eyedropper_preview.expect("a colour under the pointer");
    assert_eq!(
        ((picked.r * 255.0).round(), (picked.g * 255.0).round()),
        (200.0, 150.0)
    );
    o.on_mouse_down();
    assert_eq!(o.current_color, picked);
    // Picking a colour hands back to drawing.
    assert_eq!(o.active_tool, ToolKind::Arrow);
}

#[test]
fn right_click_cancels_only_before_a_region_exists() {
    use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
    let mut o = overlay();
    o.on_cursor_moved(Point::new(300.0, 300.0));
    assert_eq!(o.on_right_click(), Outcome::Close);

    let mut o = with_region();
    o.on_cursor_moved(Point::new(300.0, 300.0));
    assert_eq!(o.on_right_click(), Outcome::Nothing);

    // On a colour swatch it opens the picker for that preset.
    let button = BUTTONS
        .iter()
        .position(|b| b.action == ButtonAction::Color(3))
        .unwrap();
    let visible = o.visible_buttons.iter().position(|b| *b == button).unwrap();
    let (x, y, w, h) = o.toolbar().unwrap().button_rect(visible);
    o.on_cursor_moved(Point::new(x + w / 2.0, y + h / 2.0));
    assert_eq!(o.on_right_click(), Outcome::PickColor(3));

    let teal = Color::new(0.0, 0.5, 0.5, 1.0);
    o.apply_picked_color(teal);
    assert_eq!(o.current_color, teal);
}

#[test]
fn the_wheel_sizes_what_the_active_tool_draws() {
    let mut o = with_region();
    let before = o.current_thickness;
    assert_eq!(o.on_scroll(2.0), Outcome::Redraw);
    assert_eq!(o.current_thickness, before + 2.0);
    o.on_scroll(-100.0);
    assert_eq!(o.current_thickness, 1.0, "clamped");

    o.active_tool = ToolKind::StepMarker;
    let size = o.step_marker_tool.size();
    o.on_scroll(1.0);
    assert_eq!(o.step_marker_tool.size(), size + 2.0);
}

// ───────────── window capture ─────────────

#[test]
fn window_capture_selects_the_window_under_the_pointer() {
    let mut o = overlay();
    // Front to back: a small window over a large one.
    o.window_pick = Some(vec![(200, 200, 100, 80), (50, 50, 600, 400)]);
    o.on_cursor_moved(Point::new(230.0, 230.0));
    assert_eq!(
        o.selection.map(|s| (s.x, s.y, s.width, s.height)),
        Some((200.0, 200.0, 100.0, 80.0))
    );
    assert_eq!(o.cursor_icon(), CursorIcon::Crosshair);
    o.on_cursor_moved(Point::new(60.0, 60.0));
    assert_eq!(o.selection.map(|s| s.width), Some(600.0));
    o.on_cursor_moved(Point::new(10.0, 10.0));
    assert_eq!(o.selection, None);

    // The click confirms the window and hands over to the markup tools.
    o.on_cursor_moved(Point::new(60.0, 60.0));
    assert_eq!(o.on_mouse_down(), Outcome::Redraw);
    assert!(o.window_pick.is_none());
    assert_eq!(o.selection.map(|s| s.width), Some(600.0));
}

#[test]
fn window_capture_follows_the_capture_offset() {
    // The left monitor sits at negative x; windows are in screen pixels.
    let mut shot = capture();
    shot.x_offset = -400;
    let mut o = OverlayState::new(shot, &Config::default());
    o.window_pick = Some(vec![(-300, 100, 200, 150)]);
    o.on_cursor_moved(Point::new(150.0, 120.0));
    assert_eq!(
        o.selection.map(|s| (s.x, s.y)),
        Some((100.0, 100.0)),
        "screen x -300 is overlay x 100"
    );
}

// ───────────── the cursor ─────────────

#[test]
fn the_cursor_says_what_a_press_would_do() {
    let mut o = with_region();
    let at = |o: &mut OverlayState, x: f32, y: f32| {
        o.on_cursor_moved(Point::new(x, y));
        o.cursor_icon()
    };
    assert_eq!(at(&mut o, 100.0, 100.0), CursorIcon::NwseResize);
    assert_eq!(at(&mut o, 500.0, 100.0), CursorIcon::NeswResize);
    assert_eq!(at(&mut o, 300.0, 100.0), CursorIcon::NsResize);
    assert_eq!(at(&mut o, 100.0, 250.0), CursorIcon::EwResize);
    assert_eq!(at(&mut o, 300.0, 250.0), CursorIcon::Crosshair);
    assert_eq!(at(&mut o, 700.0, 50.0), CursorIcon::Crosshair);
    o.active_tool = ToolKind::Select;
    assert_eq!(at(&mut o, 300.0, 250.0), CursorIcon::Default);
    o.active_tool = ToolKind::Text;
    assert_eq!(at(&mut o, 300.0, 250.0), CursorIcon::Text);
}

// ───────────── text ─────────────

fn text_of(a: &Annotation) -> &str {
    match a {
        Annotation::Text { text, .. } => text,
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn typing_then_enter_adds_a_text_annotation() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    assert!(o.text_input_active);
    type_text(&mut o, "hello");
    // Letters are text now, not tool keys.
    assert_eq!(o.active_tool, ToolKind::Text);
    assert_eq!(key(&mut o, KeyInput::named(Named::Enter)), Outcome::Redraw);
    assert!(!o.text_input_active);
    assert_eq!(text_of(&o.annotations[0]), "hello");
    assert_eq!(o.undo_stack.len(), 1);
}

#[test]
fn the_caret_moves_and_edits_in_the_middle() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "helo");
    key(&mut o, KeyInput::named(Named::Left));
    type_text(&mut o, "l");
    assert_eq!(o.text_input_buffer, "hello");
    key(&mut o, KeyInput::named(Named::Home));
    type_text(&mut o, "> ");
    key(&mut o, KeyInput::named(Named::End));
    type_text(&mut o, "!");
    assert_eq!(o.text_input_buffer, "> hello!");
    // Delete removes what follows the caret, Backspace what precedes it.
    key(&mut o, KeyInput::named(Named::Home));
    key(&mut o, KeyInput::named(Named::Delete));
    key(&mut o, KeyInput::named(Named::End));
    key(&mut o, KeyInput::named(Named::Backspace));
    assert_eq!(o.text_input_buffer, " hello");
}

#[test]
fn shift_enter_starts_a_new_line() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "one");
    key(&mut o, KeyInput::named(Named::Enter).with(Mods::SHIFT));
    type_text(&mut o, "two");
    assert!(o.text_input_active, "Shift+Enter doesn't finish the text");
    // Up goes to the first line; typing lands there.
    key(&mut o, KeyInput::named(Named::Up));
    key(&mut o, KeyInput::named(Named::End));
    type_text(&mut o, "!");
    key(&mut o, KeyInput::named(Named::Enter));
    assert_eq!(text_of(&o.annotations[0]), "one!\ntwo");

    // The second line is drawn below the first: the bounding box grows.
    let (_, _, _, two_lines) =
        screen_mark::tools::annotation_bounding_box(&o.annotations[0]).unwrap();
    let one_line = screen_mark::tools::measure_text_height(20.0);
    assert!(two_lines > one_line * 1.8, "{two_lines} vs {one_line}");
}

#[test]
fn pasted_text_keeps_its_lines() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    // Ctrl+V asks the app for the clipboard; other Ctrl chords do nothing.
    assert_eq!(
        key(&mut o, KeyInput::typed("v").with(Mods::CTRL)),
        Outcome::Paste
    );
    assert_eq!(
        key(&mut o, KeyInput::typed("c").with(Mods::CTRL)),
        Outcome::Nothing
    );
    assert_eq!(o.insert_text("a\r\nb\tc"), Outcome::Redraw);
    assert_eq!(o.text_input_buffer, "a\nb    c");
    // Not typing: pasted text has nowhere to go.
    key(&mut o, KeyInput::named(Named::Escape));
    assert_eq!(o.insert_text("x"), Outcome::Nothing);
}

#[test]
fn escape_cancels_the_text() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "nope");
    assert_eq!(key(&mut o, KeyInput::named(Named::Escape)), Outcome::Redraw);
    assert!(!o.text_input_active);
    assert!(o.annotations.is_empty());
    assert!(o.undo_stack.is_empty());
}

#[test]
fn blank_text_adds_nothing() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "   ");
    key(&mut o, KeyInput::named(Named::Enter));
    assert!(o.annotations.is_empty());
}

#[test]
fn clicking_elsewhere_finishes_the_text_instead_of_losing_it() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "first");
    // A second click with the text tool: the first text is kept and a new
    // one starts at the click.
    click(&mut o, (200.0, 300.0));
    assert_eq!(o.annotations.len(), 1);
    assert_eq!(text_of(&o.annotations[0]), "first");
    assert!(o.text_input_active);
    type_text(&mut o, "second");

    // Copy from the toolbar includes the text still being typed.
    use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
    let button = BUTTONS
        .iter()
        .position(|b| b.action == ButtonAction::Copy)
        .unwrap();
    let visible = o.visible_buttons.iter().position(|b| *b == button).unwrap();
    let (x, y, w, h) = o.toolbar().unwrap().button_rect(visible);
    o.on_cursor_moved(Point::new(x + w / 2.0, y + h / 2.0));
    assert_eq!(o.on_mouse_down(), Outcome::Copy);
    assert_eq!(o.annotations.len(), 2);
    assert_eq!(text_of(&o.annotations[1]), "second");
}

#[test]
fn a_click_on_the_text_being_typed_moves_the_caret() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "abcdef");
    // Just inside the left edge of the text: the caret goes to the start.
    click(&mut o, (201.0, 210.0));
    assert!(o.text_input_active);
    assert!(o.annotations.is_empty(), "the click didn't finish the text");
    type_text(&mut o, ">");
    assert_eq!(o.text_input_buffer, ">abcdef");
}

#[test]
fn re_editing_a_text_is_one_undo_step_and_escape_restores_it() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "draft");
    key(&mut o, KeyInput::named(Named::Enter));

    // Select it, then click it again to edit.
    o.active_tool = ToolKind::Select;
    click(&mut o, (210.0, 210.0));
    assert_eq!(o.selected_index, Some(0));
    click(&mut o, (210.0, 210.0));
    assert!(o.text_input_active);
    assert_eq!(o.text_input_buffer, "draft");
    // The caret starts at the end.
    type_text(&mut o, "!");
    key(&mut o, KeyInput::named(Named::Escape));
    assert_eq!(text_of(&o.annotations[0]), "draft", "Escape put it back");

    click(&mut o, (210.0, 210.0));
    click(&mut o, (210.0, 210.0));
    type_text(&mut o, " 2");
    key(&mut o, KeyInput::named(Named::Enter));
    assert_eq!(text_of(&o.annotations[0]), "draft 2");
    assert_eq!(o.undo_stack.len(), 2, "add + one edit");
    key(&mut o, KeyInput::typed("z").with(Mods::CTRL));
    assert_eq!(text_of(&o.annotations[0]), "draft");
}

#[test]
fn an_input_method_composes_at_the_caret_then_commits() {
    let mut o = with_region();
    // Not typing: composition text is ignored.
    assert_eq!(o.set_preedit("ni"), Outcome::Nothing);

    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    type_text(&mut o, "ab");
    key(&mut o, KeyInput::named(Named::Left));
    assert_eq!(o.set_preedit("ni"), Outcome::Redraw);
    // Shown in place, caret after it, but not part of the text yet.
    assert_eq!(o.text_for_display(), ("anib".to_string(), 3));
    assert_eq!(o.text_input_buffer, "ab");
    assert_eq!(o.insert_text("\u{4f60}"), Outcome::Redraw);
    assert_eq!(o.text_input_buffer, "a\u{4f60}b");
    assert_eq!(o.text_preedit, "");
    key(&mut o, KeyInput::named(Named::Enter));
    assert_eq!(text_of(&o.annotations[0]), "a\u{4f60}b");
}

#[test]
fn the_caret_rect_follows_lines() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    assert_eq!(o.text_caret_rect(), None);
    click(&mut o, (200.0, 200.0));
    let (x0, y0, h) = o.text_caret_rect().unwrap();
    assert_eq!((x0, y0, h), (200.0, 200.0, 20.0));
    type_text(&mut o, "wide");
    let (x1, y1, _) = o.text_caret_rect().unwrap();
    assert!(x1 > x0 && y1 == y0);
    key(&mut o, KeyInput::named(Named::Enter).with(Mods::SHIFT));
    let (x2, y2, _) = o.text_caret_rect().unwrap();
    assert_eq!(x2, 200.0);
    assert!(y2 > y1);
}

#[test]
fn the_wheel_sizes_the_text_while_typing() {
    let mut o = with_region();
    o.active_tool = ToolKind::Text;
    click(&mut o, (200.0, 200.0));
    o.on_scroll(4.0);
    type_text(&mut o, "big");
    key(&mut o, KeyInput::named(Named::Enter));
    assert!(matches!(o.annotations[0], Annotation::Text { font_size, .. } if font_size == 24.0));
}

// ───────────── pixel-exact work: arrows and Shift ─────────────

#[test]
fn before_a_region_exists_arrows_move_the_pointer() {
    let mut o = overlay();
    o.on_cursor_moved(Point::new(300.0, 200.0));
    assert_eq!(
        key(&mut o, KeyInput::named(Named::Right)),
        Outcome::MovePointer(Point::new(301.0, 200.0))
    );
    assert_eq!(
        key(&mut o, KeyInput::named(Named::Up).with(Mods::SHIFT)),
        Outcome::MovePointer(Point::new(300.0, 190.0))
    );
    // Never off the capture.
    o.on_cursor_moved(Point::new(0.0, 0.0));
    assert_eq!(
        key(&mut o, KeyInput::named(Named::Left)),
        Outcome::MovePointer(Point::new(0.0, 0.0))
    );

    // Also while a region is being dragged out: the far corner follows.
    o.on_cursor_moved(Point::new(100.0, 100.0));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(200.0, 180.0));
    let Outcome::MovePointer(to) = key(&mut o, KeyInput::named(Named::Down)) else {
        panic!("expected the pointer to move");
    };
    o.on_cursor_moved(to);
    assert_eq!(
        o.selection.map(|s| (s.width, s.height)),
        Some((100.0, 81.0))
    );
}

#[test]
fn arrows_move_the_region_and_ctrl_arrows_resize_it() {
    let mut o = with_region(); // 400 x 300 at (100, 100)
    assert_eq!(key(&mut o, KeyInput::named(Named::Right)), Outcome::Redraw);
    key(&mut o, KeyInput::named(Named::Down).with(Mods::SHIFT));
    assert_eq!(o.selection.map(|s| (s.x, s.y)), Some((101.0, 110.0)));

    key(&mut o, KeyInput::named(Named::Right).with(Mods::CTRL));
    key(
        &mut o,
        KeyInput::named(Named::Up).with(Mods::CTRL | Mods::SHIFT),
    );
    assert_eq!(
        o.selection.map(|s| (s.x, s.y, s.width, s.height)),
        Some((101.0, 110.0, 401.0, 290.0))
    );

    // It stays on the capture.
    for _ in 0..100 {
        key(&mut o, KeyInput::named(Named::Left).with(Mods::SHIFT));
        key(
            &mut o,
            KeyInput::named(Named::Right).with(Mods::CTRL | Mods::SHIFT),
        );
    }
    assert_eq!(o.selection.map(|s| (s.x, s.width)), Some((0.0, W as f32)));
    for _ in 0..100 {
        key(
            &mut o,
            KeyInput::named(Named::Left).with(Mods::CTRL | Mods::SHIFT),
        );
    }
    assert_eq!(o.selection.map(|s| s.width), Some(3.0), "never to nothing");
}

#[test]
fn arrows_move_the_selected_annotation_as_one_undo_step() {
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    drag(&mut o, (150.0, 150.0), (250.0, 220.0));
    o.active_tool = ToolKind::Select;
    click(&mut o, (150.0, 180.0));
    assert_eq!(o.selected_index, Some(0));

    for _ in 0..5 {
        assert_eq!(key(&mut o, KeyInput::named(Named::Right)), Outcome::Redraw);
    }
    key(&mut o, KeyInput::named(Named::Down).with(Mods::SHIFT));
    let at = |o: &OverlayState| match &o.annotations[0] {
        Annotation::Rectangle { top_left, .. } => (top_left.x, top_left.y),
        other => panic!("{other:?}"),
    };
    assert_eq!(at(&o), (155.0, 160.0));
    assert_eq!(o.undo_stack.len(), 2, "add + one run of nudges");
    // The region itself didn't move.
    assert_eq!(o.selection.map(|s| (s.x, s.y)), Some((100.0, 100.0)));

    key(&mut o, KeyInput::typed("z").with(Mods::CTRL));
    assert_eq!(at(&o), (150.0, 150.0), "undo takes the whole run back");

    // Another key in between starts a new run.
    click(&mut o, (150.0, 180.0));
    key(&mut o, KeyInput::named(Named::Right));
    key(&mut o, KeyInput::typed("q"));
    key(&mut o, KeyInput::named(Named::Right));
    assert_eq!(o.undo_stack.len(), 3);
}

#[test]
fn shift_makes_squares_and_snaps_lines() {
    // A square region.
    let mut o = overlay();
    o.set_modifiers(Mods::SHIFT);
    drag(&mut o, (100.0, 100.0), (300.0, 160.0));
    assert_eq!(
        o.selection.map(|s| (s.width, s.height)),
        Some((200.0, 200.0))
    );

    // A square box, whichever way it is dragged.
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    o.set_modifiers(Mods::SHIFT);
    drag(&mut o, (300.0, 300.0), (240.0, 200.0));
    assert!(matches!(
        &o.annotations[0],
        Annotation::Rectangle { top_left, size, .. }
            if (top_left.x, top_left.y, size.width, size.height) == (200.0, 200.0, 100.0, 100.0)
    ));

    // A circle.
    o.active_tool = ToolKind::Circle;
    drag(&mut o, (150.0, 150.0), (250.0, 180.0));
    assert!(matches!(
        &o.annotations[1],
        Annotation::Ellipse { radius_x, radius_y, .. } if radius_x == radius_y
    ));

    // A line a few degrees off horizontal becomes horizontal, keeping its
    // length; near 45 degrees it becomes exactly diagonal.
    o.active_tool = ToolKind::Line;
    drag(&mut o, (150.0, 150.0), (250.0, 156.0));
    let Annotation::Line { start, end, .. } = &o.annotations[2] else {
        panic!("a line");
    };
    assert_eq!(start.y, end.y);
    assert!(
        (end.x - start.x - 100.18).abs() < 0.01,
        "{}",
        end.x - start.x
    );
    drag(&mut o, (150.0, 150.0), (250.0, 240.0));
    let Annotation::Line { start, end, .. } = &o.annotations[3] else {
        panic!("a line");
    };
    assert!(((end.x - start.x) - (end.y - start.y)).abs() < 0.01);

    // Freehand stays freehand.
    o.active_tool = ToolKind::Pencil;
    drag(&mut o, (150.0, 150.0), (250.0, 156.0));
    let Annotation::Pencil { points, .. } = &o.annotations[4] else {
        panic!("a pencil stroke");
    };
    assert_eq!(points.last().map(|p| p.y), Some(156.0));
}

#[test]
fn pressing_shift_in_the_middle_of_a_drag_takes_effect_at_once() {
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    o.on_cursor_moved(Point::new(150.0, 150.0));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(250.0, 180.0));
    let preview = |o: &OverlayState| {
        let tool = o.tool(ToolKind::Rectangle).unwrap();
        match tool.in_progress_annotation() {
            Some(Annotation::Rectangle { size, .. }) => (size.width, size.height),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(preview(&o), (100.0, 30.0));
    assert_eq!(o.set_modifiers(Mods::SHIFT), Outcome::Redraw);
    assert_eq!(preview(&o), (100.0, 100.0));
    assert_eq!(o.set_modifiers(Mods::NONE), Outcome::Redraw);
    assert_eq!(preview(&o), (100.0, 30.0));
    // No drag: a modifier on its own changes nothing.
    o.on_mouse_up();
    assert_eq!(o.set_modifiers(Mods::SHIFT), Outcome::Nothing);
}

#[test]
fn the_magnifier_shows_only_while_a_region_is_being_placed() {
    let mut o = overlay();
    assert!(o.magnifier_visible(), "before a region exists");
    o.on_cursor_moved(Point::new(100.0, 100.0));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(500.0, 400.0));
    assert!(o.magnifier_visible(), "while dragging one out");
    o.on_mouse_up();
    assert!(!o.magnifier_visible(), "not while marking up");
    // Resizing by an edge brings it back.
    o.on_cursor_moved(Point::new(500.0, 250.0));
    o.on_mouse_down();
    assert!(o.magnifier_visible());
    o.on_mouse_up();
    assert!(!o.magnifier_visible());

    let mut config = Config::default();
    config.general.magnifier = false;
    assert!(!OverlayState::new(capture(), &config).magnifier_visible());
    let mut picking = overlay();
    picking.window_pick = Some(vec![]);
    assert!(!picking.magnifier_visible(), "not when picking a window");
}

#[test]
fn arrows_move_the_pointer_while_a_window_is_being_picked() {
    let mut o = overlay();
    o.window_pick = Some(vec![(50, 50, 600, 400)]);
    o.on_cursor_moved(Point::new(60.0, 60.0));
    assert!(
        o.selection.is_some(),
        "the window under the pointer is outlined"
    );
    // The outline is not a region to nudge yet.
    assert_eq!(
        key(&mut o, KeyInput::named(Named::Right)),
        Outcome::MovePointer(Point::new(61.0, 60.0))
    );
    assert_eq!(o.selection.map(|s| s.x), Some(50.0));
}
