//! Editing the text of a text annotation: a string and a caret.
//!
//! Lines are separated by `\n`. The caret is a byte offset that always sits
//! on a grapheme boundary, so an emoji or a letter with a combining accent
//! is stepped over, and deleted, as one.

use unicode_segmentation::UnicodeSegmentation;

/// Insert `s` at the caret and move the caret past it. Line endings become
/// `\n`, a tab becomes four spaces, other control characters are dropped.
pub fn insert(text: &mut String, caret: &mut usize, s: &str) {
    let clean = clean(s);
    let at = snap(text, *caret);
    text.insert_str(at, &clean);
    *caret = at + clean.len();
}

fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The nearest grapheme boundary at or before `caret`.
pub fn snap(text: &str, caret: usize) -> usize {
    let caret = caret.min(text.len());
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .take_while(|i| *i <= caret)
        .last()
        .unwrap_or(0)
}

/// Delete the grapheme before the caret. False if there is none.
pub fn backspace(text: &mut String, caret: &mut usize) -> bool {
    let at = snap(text, *caret);
    let Some((start, _)) = text[..at].grapheme_indices(true).next_back() else {
        *caret = at;
        return false;
    };
    text.replace_range(start..at, "");
    *caret = start;
    true
}

/// Delete the grapheme after the caret. False if there is none.
pub fn delete(text: &mut String, caret: &mut usize) -> bool {
    let at = snap(text, *caret);
    *caret = at;
    let Some(next) = text[at..].graphemes(true).next() else {
        return false;
    };
    let end = at + next.len();
    text.replace_range(at..end, "");
    true
}

/// Move the caret one grapheme left. False at the start.
pub fn left(text: &str, caret: &mut usize) -> bool {
    let at = snap(text, *caret);
    match text[..at].grapheme_indices(true).next_back() {
        Some((start, _)) => {
            *caret = start;
            true
        }
        None => false,
    }
}

/// Move the caret one grapheme right. False at the end.
pub fn right(text: &str, caret: &mut usize) -> bool {
    let at = snap(text, *caret);
    match text[at..].graphemes(true).next() {
        Some(next) => {
            *caret = at + next.len();
            true
        }
        None => false,
    }
}

/// The byte range of the line holding the caret, without its `\n`.
pub fn line_range(text: &str, caret: usize) -> (usize, usize) {
    let at = snap(text, caret);
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, end)
}

/// The caret as (line number, byte offset within that line).
pub fn line_col(text: &str, caret: usize) -> (usize, usize) {
    let at = snap(text, caret);
    let (start, _) = line_range(text, at);
    (text[..start].matches('\n').count(), at - start)
}

/// Move the caret to the start of its line. False if it was there.
pub fn home(text: &str, caret: &mut usize) -> bool {
    let (start, _) = line_range(text, *caret);
    let moved = *caret != start;
    *caret = start;
    moved
}

/// Move the caret to the end of its line. False if it was there.
pub fn end(text: &str, caret: &mut usize) -> bool {
    let (_, end) = line_range(text, *caret);
    let moved = *caret != end;
    *caret = end;
    moved
}

/// The offset in `line` whose left edge is nearest to `x`, where `width`
/// measures a piece of text.
pub fn offset_at_x(line: &str, x: f32, width: &dyn Fn(&str) -> f32) -> usize {
    let mut best = (0, x.abs());
    for (i, g) in line.grapheme_indices(true) {
        let end = i + g.len();
        let d = (width(&line[..end]) - x).abs();
        if d < best.1 {
            best = (end, d);
        }
    }
    best.0
}

/// Move the caret to the line above (`up`) or below, keeping its horizontal
/// position as closely as the text allows. False on the first or last line.
pub fn vertical(text: &str, caret: &mut usize, up: bool, width: &dyn Fn(&str) -> f32) -> bool {
    let at = snap(text, *caret);
    let (start, end) = line_range(text, at);
    let x = width(&text[start..at]);
    let target = if up {
        if start == 0 {
            return false;
        }
        line_range(text, start - 1)
    } else {
        if end == text.len() {
            return false;
        }
        line_range(text, end + 1)
    };
    *caret = target.0 + offset_at_x(&text[target.0..target.1], x, width);
    true
}

/// The caret position for a click at (`x`, `y`) relative to the text's
/// top-left corner, with lines `line_height` apart.
pub fn offset_at_point(
    text: &str,
    x: f32,
    y: f32,
    line_height: f32,
    width: &dyn Fn(&str) -> f32,
) -> usize {
    let lines: Vec<&str> = text.split('\n').collect();
    let row = if line_height > 0.0 {
        ((y / line_height).floor().max(0.0) as usize).min(lines.len() - 1)
    } else {
        0
    };
    let before: usize = lines[..row].iter().map(|l| l.len() + 1).sum();
    before + offset_at_x(lines[row], x, width)
}
