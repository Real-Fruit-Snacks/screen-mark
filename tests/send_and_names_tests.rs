//! The `file_name` pattern for saved captures, and Send: handing a capture
//! to a command of your own.

use std::path::Path;
use std::time::Duration;

use chrono::TimeZone;
use screen_mark::config::Config;
use screen_mark::export::{file_stem, DEFAULT_FILE_NAME};
use screen_mark::send;

fn when() -> chrono::DateTime<chrono::Local> {
    chrono::Local
        .with_ymd_and_hms(2026, 10, 28, 14, 30, 12)
        .unwrap()
}

// ───────────── file names ─────────────

#[test]
fn the_default_name_is_the_one_screen_mark_always_used() {
    assert_eq!(
        file_stem(DEFAULT_FILE_NAME, when(), 10, 10),
        "screen-mark_2026-10-28_143012"
    );
}

#[test]
fn every_placeholder_is_filled_in() {
    assert_eq!(
        file_stem(
            "{year}{month}{day}-{hour}{minute}{second} {width}x{height} {date} {time}",
            when(),
            1920,
            1080
        ),
        "20261028-143012 1920x1080 2026-10-28 143012"
    );
    assert_eq!(
        file_stem("shot {nope}", when(), 1, 1),
        "shot {nope}",
        "unknown kept"
    );
}

#[test]
fn a_pattern_never_names_a_folder_or_a_bad_file() {
    assert_eq!(
        file_stem("a/b\\c:d*e?f\"g<h>i|j", when(), 1, 1),
        "a-b-c-d-e-f-g-h-i-j"
    );
    assert_eq!(
        file_stem("  trailing dots...  ", when(), 1, 1),
        "trailing dots"
    );
    assert_eq!(
        file_stem("named.png", when(), 1, 1),
        "named",
        ".png isn't doubled"
    );
    // Nothing left: the default.
    for empty in ["", "   ", "///", "..."] {
        assert_eq!(
            file_stem(empty, when(), 1, 1),
            "screen-mark_2026-10-28_143012",
            "{empty:?}"
        );
    }
}

#[test]
fn file_name_and_send_have_defaults_and_read_from_the_config() {
    let config = Config::default();
    assert_eq!(config.general.file_name, DEFAULT_FILE_NAME);
    assert_eq!(config.general.shape_style, "outline");
    assert!(config.send.command.is_empty());
    assert_eq!(config.keys.style, "d");
    assert!(config.keys.send.is_empty());
    let config: Config = toml::from_str(
        "[general]\nfile_name = \"{date} shot\"\nshape_style = \"dashed\"\n\
         [send]\ncommand = \"upload.sh --quiet\"\nname = \"Share\"\n\
         [keys]\nsend = \"Ctrl+U\"\n",
    )
    .unwrap();
    assert_eq!(config.general.file_name, "{date} shot");
    assert_eq!(config.general.shape_style, "dashed");
    assert_eq!(config.send.command, "upload.sh --quiet");
    assert_eq!(send::name(&config.send), "Share");
    assert_eq!(send::name(&Config::default().send), "Send");
    assert!(config.key_report().invalid.is_empty());
    assert!(config.key_report().conflicts.is_empty());
}

#[test]
fn the_style_key_is_a_bare_key_like_the_tools() {
    let report = Config::default().key_report();
    assert!(report.invalid.is_empty(), "{:?}", report.invalid);
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    // Bound to a tool's key, it conflicts with that tool.
    let mut config = Config::default();
    config.keys.style = "a".into();
    assert!(!config.key_report().conflicts.is_empty());
}

// ───────────── Send ─────────────

#[test]
fn a_command_splits_like_a_shell_would_without_one() {
    assert_eq!(
        send::split(r#"upload "my script.sh" --to 'a b' C:\tools\x.exe"#).unwrap(),
        ["upload", "my script.sh", "--to", "a b", r"C:\tools\x.exe"]
    );
    assert!(send::split("  ").is_err());
    let e = send::split("echo \"open").unwrap_err();
    assert!(e.contains("isn't closed"), "{e}");
}

#[test]
fn the_file_goes_where_file_is_or_last() {
    let file = Path::new("/tmp/shot 1.png");
    let (program, args) = send::arguments("scp {file} host:", file).unwrap();
    assert_eq!(program, "scp");
    assert_eq!(args, ["/tmp/shot 1.png", "host:"]);
    let (_, args) = send::arguments("curl -F file=@{file} https://x", file).unwrap();
    assert_eq!(args, ["-F", "file=@/tmp/shot 1.png", "https://x"]);
    let (_, args) = send::arguments("upload --public", file).unwrap();
    assert_eq!(args, ["--public", "/tmp/shot 1.png"]);
}

/// A command that prints `words` and its last argument, on this platform.
fn echo(words: &str) -> String {
    if cfg!(windows) {
        format!("cmd /C echo {words}")
    } else {
        format!("sh -c 'echo {words} \"$0\"'")
    }
}

#[test]
fn what_the_command_prints_comes_back() {
    let file = Path::new("shot.png");
    let out = send::run(
        &echo("https://example.com/abc"),
        file,
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(out.starts_with("https://example.com/abc"), "{out:?}");
    assert!(out.ends_with("shot.png"), "the file was passed: {out:?}");
}

#[test]
fn a_failing_command_says_how_it_failed() {
    let file = Path::new("shot.png");
    let fail = if cfg!(windows) {
        "cmd /C \"echo nope 1>&2 && exit 3\""
    } else {
        "sh -c 'echo nope >&2; exit 3'"
    };
    let e = send::run(fail, file, Duration::from_secs(20)).unwrap_err();
    assert!(e.contains("exit code 3") && e.contains("nope"), "{e}");
    let e = send::run("no-such-program-sm", file, Duration::from_secs(20)).unwrap_err();
    assert!(e.contains("wasn't found"), "{e}");
}

#[test]
fn a_command_that_hangs_is_stopped() {
    // The file is passed last; rem and $0 swallow it.
    let slow = if cfg!(windows) {
        "cmd /C ping -n 4 127.0.0.1 >NUL & rem"
    } else {
        "sh -c 'sleep 4'"
    };
    let started = std::time::Instant::now();
    let e = send::run(slow, Path::new("x.png"), Duration::from_millis(400)).unwrap_err();
    assert!(e.contains("still running"), "{e}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_sent_capture_is_a_real_png_in_the_send_folder() {
    let pixels = [10u8, 20, 30, 255].repeat(6);
    let file = send::write_png(&pixels, 3, 2).unwrap();
    assert!(file.starts_with(send::folder()));
    let back = image::open(&file).unwrap().to_rgba8();
    assert_eq!(back.dimensions(), (3, 2));
    assert_eq!(back.get_pixel(0, 0).0, [10, 20, 30, 255]);
    let _ = std::fs::remove_file(file);
}
