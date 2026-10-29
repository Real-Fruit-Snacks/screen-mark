//! Key bindings: parsing, the rules per scope, layout handling, conflicts,
//! and what the overlay does with a key press.

use global_hotkey::hotkey::{Code, Modifiers};
use screen_mark::keys::{
    caution, check, key_from_winit, typed_char, validate, Binding, Key, KeyId, KeyMap, Mods, Named,
    Scope,
};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, NativeKeyCode, PhysicalKey, SmolStr};

fn b(s: &str) -> Binding {
    Binding::parse(s).unwrap().unwrap()
}

fn ch(c: &str) -> WKey {
    WKey::Character(SmolStr::new(c))
}

fn code(k: KeyCode) -> PhysicalKey {
    PhysicalKey::Code(k)
}

// ───────────── parsing and display ─────────────

#[test]
fn parses_modifiers_in_any_order_and_case() {
    let x = b("shift+CTRL+s");
    assert_eq!(x.mods, Mods::CTRL | Mods::SHIFT);
    assert_eq!(x.key, Key::Char('s'));
    assert_eq!(x.to_string(), "Ctrl+Shift+S");
    assert_eq!(b("  Ctrl + Alt + P ").to_string(), "Ctrl+Alt+P");
}

#[test]
fn parses_aliases() {
    assert_eq!(b("Control+PrtSc"), b("Ctrl+PrintScreen"));
    assert_eq!(b("Ctrl+Print"), b("Ctrl+PrintScreen"));
    assert_eq!(b("Win+Shift+S").mods, Mods::SUPER | Mods::SHIFT);
    assert_eq!(b("Meta+X"), b("Super+X"));
    assert_eq!(b("Ctrl+Escape"), b("Ctrl+Esc"));
    assert_eq!(b("Ctrl+Shift+-"), b("Ctrl+Shift+Minus"));
    assert_eq!(b("Ctrl+PgUp"), b("Ctrl+PageUp"));
    assert_eq!(b("Ctrl+Alt+numpad5"), b("Ctrl+Alt+Num5"));
}

#[test]
fn empty_means_unbound() {
    assert_eq!(Binding::parse("").unwrap(), None);
    assert_eq!(Binding::parse("   ").unwrap(), None);
}

#[test]
fn rejects_malformed_bindings_with_a_reason() {
    for bad in [
        "Ctrl+",
        "Ctrl+Shift",
        "Ctrl+Foo",
        "S+Ctrl",
        "Ctrl+A+B",
        "Ctrl++",
    ] {
        let err = Binding::parse(bad).expect_err(bad);
        assert!(err.contains(bad.trim()), "{bad}: {err}");
    }
}

#[test]
fn every_key_round_trips_through_its_name() {
    let mut keys: Vec<Key> = Named::ALL.iter().map(|n| Key::Named(*n)).collect();
    keys.extend(('a'..='z').chain('0'..='9').map(Key::Char));
    for key in keys {
        for mods in [
            Mods::NONE,
            Mods::CTRL | Mods::ALT,
            Mods::CTRL | Mods::SHIFT | Mods::ALT | Mods::SUPER,
        ] {
            let x = Binding::new(mods, key);
            assert_eq!(b(&x.to_string()), x, "{x}");
        }
    }
}

#[test]
fn converts_to_global_hotkey_codes() {
    let hk = b("Ctrl+PrintScreen").to_hotkey();
    assert_eq!(hk.mods, Modifiers::CONTROL);
    assert_eq!(hk.key, Code::PrintScreen);
    let hk = b("Ctrl+Shift+S").to_hotkey();
    assert_eq!(hk.mods, Modifiers::CONTROL | Modifiers::SHIFT);
    assert_eq!(hk.key, Code::KeyS);
    assert_eq!(b("Super+Alt+7").to_hotkey().key, Code::Digit7);
    assert_eq!(b("Ctrl+Alt+Num5").to_hotkey().key, Code::Numpad5);
    assert_eq!(b("F13").to_hotkey().key, Code::F13);
}

// ───────────── rules ─────────────

#[test]
fn global_hotkeys_need_ctrl_alt_or_win_on_typing_keys() {
    for ok in [
        "Ctrl+Shift+S",
        "Ctrl+Alt+P",
        "Win+S",
        "PrintScreen",
        "Ctrl+PrintScreen",
        "Ctrl+F5",
        "F13",
        "Pause",
        "Alt+Insert",
        // What 2.0 accepted keeps working (with a caution in Settings).
        "Alt+S",
        "Ctrl+Q",
        "Ctrl+1",
        "F9",
        "Shift+F5",
    ] {
        assert!(
            validate(b(ok), Scope::Global).is_ok(),
            "{ok} should be allowed"
        );
    }
    for bad in [
        "Shift+S",
        "S",
        "Shift+1",
        "1",
        "Esc",
        "Space",
        "Shift+Space",
        "Left",
    ] {
        let err = validate(b(bad), Scope::Global).expect_err(bad);
        assert!(err.contains("needs Ctrl, Alt or Win"), "{bad}: {err}");
    }
}

#[test]
fn global_hotkeys_refuse_punctuation() {
    // global-hotkey registers these by their US character, which is a
    // different key on other layouts.
    for bad in ["Ctrl+Minus", "Ctrl+Shift+Minus", "Win+Slash", "Minus"] {
        let err = validate(b(bad), Scope::Global).expect_err(bad);
        assert!(err.contains("keyboard layouts"), "{bad}: {err}");
    }
}

#[test]
fn one_modifier_and_bare_f_keys_come_with_a_caution() {
    for warn in ["Alt+S", "Ctrl+Q", "Ctrl+1", "F5", "Shift+F5"] {
        let c = caution(b(warn), Scope::Global).unwrap_or_else(|| panic!("{warn}"));
        assert!(c.contains("Other apps"), "{warn}: {c}");
    }
    for quiet in [
        "Ctrl+Shift+S",
        "Win+S",
        "Ctrl+F5",
        "PrintScreen",
        "Ctrl+PrintScreen",
    ] {
        assert_eq!(caution(b(quiet), Scope::Global), None, "{quiet}");
    }
    assert_eq!(caution(b("Ctrl+Q"), Scope::Editor), None);
}

#[test]
fn global_hotkeys_refuse_system_combinations() {
    for sys in [
        "Alt+F4",
        "Alt+Tab",
        "Ctrl+Alt+Delete",
        "Ctrl+Shift+Esc",
        "Win+L",
    ] {
        let err = validate(b(sys), Scope::Global).unwrap_err();
        assert!(err.contains("system"), "{sys}: {err}");
    }
}

#[test]
fn editor_actions_need_a_command_modifier_or_a_named_key() {
    for ok in [
        "Ctrl+C",
        "Alt+P",
        "Ctrl+Shift+Z",
        "F2",
        "Insert",
        "Ctrl+Enter",
        "Ctrl+Minus",
    ] {
        assert!(validate(b(ok), Scope::Editor).is_ok(), "{ok}");
    }
    for bad in [
        "C",
        "Shift+C",
        "Minus",
        "Enter",
        "Esc",
        "Delete",
        "Backspace",
        "Space",
    ] {
        assert!(validate(b(bad), Scope::Editor).is_err(), "{bad}");
    }
}

#[test]
fn tool_keys_are_single_keys() {
    for ok in ["V", "1", "Comma", "F2"] {
        assert!(validate(b(ok), Scope::Tool).is_ok(), "{ok}");
    }
    for bad in [
        "Ctrl+V",
        "Shift+V",
        "Alt+1",
        "Enter",
        "Esc",
        "Delete",
        "Space",
        "Tab",
        "PrintScreen",
    ] {
        assert!(validate(b(bad), Scope::Tool).is_err(), "{bad}");
    }
}

#[test]
fn printscreen_is_only_a_global_hotkey() {
    // Windows never delivers its key-down to a window.
    for (combo, scope) in [
        ("Ctrl+PrintScreen", Scope::Editor),
        ("PrintScreen", Scope::Tool),
    ] {
        let err = validate(b(combo), scope).unwrap_err();
        assert!(err.contains("only works as a global hotkey"), "{err}");
    }
}

#[test]
fn tool_keys_from_2_0_can_be_any_typed_character() {
    // 2.0 stored whatever character was typed.
    assert_eq!(b("#").key, Key::Typed('#'));
    assert_eq!(b("+").key, Key::Typed('+'));
    assert_eq!(b("ö").key, Key::Typed('ö'));
    assert_eq!(b("Ö").key, Key::Typed('ö'));
    assert_eq!(b("<").key, Key::Typed('<'));
    // Characters with a key name keep meaning that key.
    assert_eq!(b("-").key, Key::Named(Named::Minus));
    assert_eq!(b("/").key, Key::Named(Named::Slash));

    assert!(validate(b("#"), Scope::Tool).is_ok());
    assert!(validate(b("Ctrl+#"), Scope::Editor).is_err());
    assert!(validate(b("Ctrl+#"), Scope::Global).is_err());

    // The overlay matches them by the character a press types.
    let map = KeyMap::from_report(&defaults_with(&[(KeyId::Tool(7), "#")]));
    let typed = typed_char(&ch("#")).unwrap();
    assert_eq!(map.tool(Mods::SHIFT, Key::Typed(typed)), Some(7));
    assert_eq!(typed_char(&ch("Ö")), Some('ö'));
    assert_eq!(typed_char(&WKey::Named(NamedKey::Enter)), None);
}

// ───────────── keyboard layouts ─────────────

#[test]
fn letters_follow_the_layout() {
    // US: the S key types s.
    assert_eq!(
        key_from_winit(&ch("s"), &code(KeyCode::KeyS)),
        Some(Key::Char('s'))
    );
    // With Shift (or Caps Lock) the character is uppercase; still s.
    assert_eq!(
        key_from_winit(&ch("S"), &code(KeyCode::KeyS)),
        Some(Key::Char('s'))
    );
    // AZERTY: the key in the US "A" position types q.
    assert_eq!(
        key_from_winit(&ch("q"), &code(KeyCode::KeyA)),
        Some(Key::Char('q'))
    );
    // Ctrl can turn the character into a control code; position decides.
    assert_eq!(
        key_from_winit(&ch("\u{13}"), &code(KeyCode::KeyS)),
        Some(Key::Char('s'))
    );
}

#[test]
fn non_latin_layouts_fall_back_to_position() {
    // Russian: the US "A" position types ф.
    assert_eq!(
        key_from_winit(&ch("\u{444}"), &code(KeyCode::KeyA)),
        Some(Key::Char('a'))
    );
}

#[test]
fn digits_and_punctuation_go_by_position() {
    assert_eq!(
        key_from_winit(&ch("!"), &code(KeyCode::Digit1)),
        Some(Key::Char('1'))
    );
    // AZERTY: the 1 key types & without Shift.
    assert_eq!(
        key_from_winit(&ch("&"), &code(KeyCode::Digit1)),
        Some(Key::Char('1'))
    );
    assert_eq!(
        key_from_winit(&ch("_"), &code(KeyCode::Minus)),
        Some(Key::Named(Named::Minus))
    );
    assert_eq!(
        key_from_winit(&ch("5"), &code(KeyCode::Numpad5)),
        Some(Key::Named(Named::Num5))
    );
}

#[test]
fn named_keys_and_modifiers() {
    assert_eq!(
        key_from_winit(
            &WKey::Named(NamedKey::PrintScreen),
            &code(KeyCode::PrintScreen)
        ),
        Some(Key::Named(Named::PrintScreen))
    );
    // No scancode (remapped key): the logical name still works.
    assert_eq!(
        key_from_winit(
            &WKey::Named(NamedKey::F13),
            &PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
        ),
        Some(Key::Named(Named::F13))
    );
    assert_eq!(
        key_from_winit(&WKey::Named(NamedKey::Control), &code(KeyCode::ControlLeft)),
        None
    );
    assert!(screen_mark::keys::is_modifier_key(&WKey::Named(
        NamedKey::Shift
    )));
    assert!(!screen_mark::keys::is_modifier_key(&WKey::Named(
        NamedKey::F1
    )));
    assert_eq!(
        key_from_winit(
            &WKey::Named(NamedKey::AudioVolumeUp),
            &code(KeyCode::AudioVolumeUp)
        ),
        None
    );
}

// ───────────── conflicts ─────────────

fn defaults_with(changes: &[(KeyId, &str)]) -> screen_mark::keys::Report {
    let changes: Vec<(KeyId, String)> = changes.iter().map(|(i, s)| (*i, s.to_string())).collect();
    check(|id| {
        changes
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| id.default_binding().to_string())
    })
}

#[test]
fn defaults_have_no_problems() {
    let r = defaults_with(&[]);
    assert!(r.is_ok(), "{:?} {:?}", r.invalid, r.conflicts);
    // Pin, OCR and Upload start unbound.
    assert_eq!(r.binding(KeyId::Pin), None);
    assert_eq!(r.binding(KeyId::Region), Some(b("Ctrl+Shift+S")));
    assert_eq!(r.binding(KeyId::Screenshot), Some(b("Ctrl+PrintScreen")));
}

#[test]
fn two_editor_actions_on_one_combo_conflict() {
    let r = defaults_with(&[(KeyId::Copy, "Ctrl+S")]);
    assert_eq!(r.conflicts.len(), 1);
    assert!(r.problem(KeyId::Copy).unwrap().contains("Save"));
    assert!(r.problem(KeyId::Save).unwrap().contains("Copy"));
}

#[test]
fn a_global_hotkey_on_an_editor_combo_conflicts() {
    let r = defaults_with(&[(KeyId::Region, "Ctrl+Shift+Z")]);
    assert!(r.problem(KeyId::Region).unwrap().contains("Redo"));
}

#[test]
fn the_two_global_hotkeys_cannot_share_a_combo() {
    let r = defaults_with(&[(KeyId::Screenshot, "Ctrl+Shift+S")]);
    assert!(r
        .problem(KeyId::Screenshot)
        .unwrap()
        .contains("Capture a region"));
}

#[test]
fn tool_keys_conflict_ignoring_case() {
    let r = defaults_with(&[(KeyId::Tool(1), "v")]);
    assert!(r.problem(KeyId::Tool(1)).unwrap().contains("Select"));
}

#[test]
fn a_bare_named_key_used_by_a_tool_and_an_action_conflicts() {
    let r = defaults_with(&[(KeyId::Tool(0), "F2"), (KeyId::Pin, "F2")]);
    assert!(r.problem(KeyId::Pin).is_some());
}

#[test]
fn a_tool_letter_and_ctrl_plus_that_letter_do_not_conflict() {
    // V picks Select; Ctrl+V is free for an action.
    let r = defaults_with(&[(KeyId::Pin, "Ctrl+V")]);
    assert!(r.is_ok(), "{:?}", r.conflicts);
}

#[test]
fn invalid_bindings_are_reported_not_used() {
    let r = defaults_with(&[(KeyId::Region, "Shift+S"), (KeyId::Undo, "Ctrl+Bogus")]);
    assert!(r
        .problem(KeyId::Region)
        .unwrap()
        .contains("needs Ctrl, Alt or Win"));
    assert!(r.problem(KeyId::Undo).unwrap().contains("unknown key"));
    assert_eq!(r.binding(KeyId::Region), None);
    assert_eq!(r.first_problem().unwrap().0, KeyId::Region);
}

// ───────────── matching in the editor ─────────────

#[test]
fn editor_actions_need_the_exact_modifiers() {
    let map = KeyMap::from_report(&defaults_with(&[]));
    let z = Key::Char('z');
    assert_eq!(map.editor_action(Mods::CTRL, z), Some(KeyId::Undo));
    assert_eq!(
        map.editor_action(Mods::CTRL | Mods::SHIFT, z),
        Some(KeyId::Redo)
    );
    assert_eq!(map.editor_action(Mods::CTRL | Mods::ALT, z), None);
    assert_eq!(
        map.editor_action(Mods::CTRL | Mods::SHIFT, Key::Char('c')),
        None
    );
}

#[test]
fn tools_ignore_shift_but_not_ctrl() {
    let map = KeyMap::from_report(&defaults_with(&[]));
    assert_eq!(map.tool(Mods::NONE, Key::Char('a')), Some(1));
    assert_eq!(map.tool(Mods::SHIFT, Key::Char('a')), Some(1));
    assert_eq!(map.tool(Mods::CTRL, Key::Char('a')), None);
}

#[test]
fn rebound_keys_take_effect_and_unbound_keys_do_nothing() {
    let map = KeyMap::from_report(&defaults_with(&[
        (KeyId::Copy, "Ctrl+Alt+C"),
        (KeyId::Tool(1), "Q"),
        (KeyId::Undo, ""),
    ]));
    assert_eq!(map.editor_action(Mods::CTRL, Key::Char('c')), None);
    assert_eq!(
        map.editor_action(Mods::CTRL | Mods::ALT, Key::Char('c')),
        Some(KeyId::Copy)
    );
    assert_eq!(map.tool(Mods::NONE, Key::Char('a')), None);
    assert_eq!(map.tool(Mods::NONE, Key::Char('q')), Some(1));
    assert_eq!(map.editor_action(Mods::CTRL, Key::Char('z')), None);
}
