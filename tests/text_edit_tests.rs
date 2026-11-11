//! The text annotation's editing model: a string and a caret.

use screen_mark::overlay::text_edit::{
    backspace, delete, end, home, insert, left, line_col, line_range, offset_at_point, right, snap,
    vertical,
};

/// Every character is 10 wide: positions are easy to reason about.
fn width(s: &str) -> f32 {
    s.chars().count() as f32 * 10.0
}

#[test]
fn insert_goes_in_at_the_caret() {
    let mut text = String::from("ad");
    let mut caret = 1;
    insert(&mut text, &mut caret, "bc");
    assert_eq!((text.as_str(), caret), ("abcd", 3));
}

#[test]
fn insert_normalises_line_endings_and_drops_control_characters() {
    let mut text = String::new();
    let mut caret = 0;
    insert(&mut text, &mut caret, "a\r\nb\rc\nd\te\u{7}f");
    assert_eq!(text, "a\nb\nc\nd    ef");
    assert_eq!(caret, text.len());
}

#[test]
fn backspace_and_delete_take_one_grapheme() {
    // "e" + combining acute, then a family emoji joined by ZWJ.
    let mut text = String::from("e\u{301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}x");
    let mut caret = text.len() - 1;
    assert!(backspace(&mut text, &mut caret));
    assert_eq!(text, "e\u{301}x", "the whole emoji went");
    assert!(backspace(&mut text, &mut caret));
    assert_eq!(
        (text.as_str(), caret),
        ("x", 0),
        "letter and accent together"
    );
    assert!(
        !backspace(&mut text, &mut caret),
        "nothing before the caret"
    );
    assert!(delete(&mut text, &mut caret));
    assert_eq!(text, "");
    assert!(!delete(&mut text, &mut caret));
}

#[test]
fn left_and_right_step_over_whole_graphemes() {
    let text = "a\u{1F600}b";
    let mut caret = 0;
    assert!(right(text, &mut caret));
    assert_eq!(caret, 1);
    assert!(right(text, &mut caret));
    assert_eq!(caret, 5, "past the four-byte emoji");
    assert!(right(text, &mut caret));
    assert!(!right(text, &mut caret), "at the end");
    assert!(left(text, &mut caret));
    assert!(left(text, &mut caret));
    assert_eq!(caret, 1);
    assert!(left(text, &mut caret));
    assert!(!left(text, &mut caret), "at the start");
}

#[test]
fn a_caret_inside_a_character_snaps_back_to_its_start() {
    let text = "a\u{1F600}b";
    assert_eq!(snap(text, 3), 1);
    assert_eq!(snap(text, 99), text.len());
}

#[test]
fn home_and_end_stay_on_the_line() {
    let text = "one\ntwo\nthree";
    let mut caret = 5; // "tw|o"
    assert_eq!(line_range(text, caret), (4, 7));
    assert_eq!(line_col(text, caret), (1, 1));
    assert!(home(text, &mut caret));
    assert_eq!(caret, 4);
    assert!(!home(text, &mut caret));
    assert!(end(text, &mut caret));
    assert_eq!(caret, 7);
    assert!(!end(text, &mut caret));
}

#[test]
fn up_and_down_keep_the_column_where_the_line_is_long_enough() {
    let text = "abcdef\nab\nabcdef";
    let mut caret = 4; // first line, after "abcd"
    assert!(vertical(text, &mut caret, false, &width));
    assert_eq!(caret, 9, "the short line ends at 'ab'");
    assert!(vertical(text, &mut caret, false, &width));
    assert_eq!(caret, 12, "column 2 of the last line");
    assert!(!vertical(text, &mut caret, false, &width), "no line below");
    assert!(vertical(text, &mut caret, true, &width));
    assert!(vertical(text, &mut caret, true, &width));
    assert_eq!(caret, 2);
    assert!(!vertical(text, &mut caret, true, &width), "no line above");
}

#[test]
fn a_click_lands_on_the_nearest_caret_position() {
    let text = "abcd\nef";
    // Line height 20. x = 14 is nearer to after "a" (10) than after "ab" (20).
    assert_eq!(offset_at_point(text, 14.0, 5.0, 20.0, &width), 1);
    assert_eq!(offset_at_point(text, 16.0, 5.0, 20.0, &width), 2);
    assert_eq!(
        offset_at_point(text, 500.0, 5.0, 20.0, &width),
        4,
        "past the end"
    );
    assert_eq!(
        offset_at_point(text, 11.0, 25.0, 20.0, &width),
        6,
        "second line"
    );
    assert_eq!(
        offset_at_point(text, -5.0, 400.0, 20.0, &width),
        5,
        "below: last line"
    );
}
