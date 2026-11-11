//! The log file: where it is, that it is appended to, and that it can't
//! grow without bound.

use std::io::{Read, Write};

use screen_mark::logging::{log_path, open_log, panic_message, MAX_LOG_BYTES};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sm-log-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn read(path: &std::path::Path) -> String {
    let mut s = String::new();
    std::fs::File::open(path)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn the_log_is_created_with_its_folder_and_appended_to() {
    let dir = temp_dir("append");
    let path = dir.join("nested").join("screen-mark.log");
    {
        let mut log = open_log(&path).expect("creates the folder and the file");
        writeln!(log, "first run").unwrap();
    }
    {
        let mut log = open_log(&path).unwrap();
        writeln!(log, "second run").unwrap();
    }
    assert_eq!(read(&path), "first run\nsecond run\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_full_log_is_set_aside_and_a_new_one_started() {
    let dir = temp_dir("rotate");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("screen-mark.log");
    let old = dir.join("screen-mark.log.old");

    // Just under the limit: left alone.
    std::fs::write(&path, vec![b'a'; MAX_LOG_BYTES as usize]).unwrap();
    drop(open_log(&path).unwrap());
    assert!(!old.exists());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_LOG_BYTES);

    // Over it: moved to .old, and the log starts empty.
    std::fs::write(&path, vec![b'b'; MAX_LOG_BYTES as usize + 1]).unwrap();
    {
        let mut log = open_log(&path).unwrap();
        writeln!(log, "fresh").unwrap();
    }
    assert_eq!(read(&path), "fresh\n");
    assert_eq!(std::fs::metadata(&old).unwrap().len(), MAX_LOG_BYTES + 1);

    // The next full log replaces the old one; only two files ever exist.
    std::fs::write(&path, vec![b'c'; MAX_LOG_BYTES as usize + 5]).unwrap();
    drop(open_log(&path).unwrap());
    assert_eq!(std::fs::metadata(&old).unwrap().len(), MAX_LOG_BYTES + 5);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_log_follows_screen_mark_home_and_the_crash_dialog_names_it() {
    let before = std::env::var_os("SCREEN_MARK_HOME");
    let home = temp_dir("home");
    std::env::set_var("SCREEN_MARK_HOME", &home);
    let path = log_path();
    let message = panic_message();
    // Start logging while the home is set: the file is opened now.
    screen_mark::logging::init();
    screen_mark::logging::install_panic_hook();
    match before {
        Some(old) => std::env::set_var("SCREEN_MARK_HOME", old),
        None => std::env::remove_var("SCREEN_MARK_HOME"),
    }
    let path = path.expect("a log path");
    assert_eq!(path, home.join("screen-mark.log"));
    assert!(message.contains(&path.display().to_string()), "{message}");

    // What the app logs lands in the file, and so does a panic on any
    // thread, with where it happened.
    tracing::info!("capture finished");
    tracing::debug!("not at the default level");
    let worker = std::thread::spawn(|| panic!("the worker fell over"));
    assert!(worker.join().is_err());
    let log = read(&path);
    assert!(
        log.contains("INFO") && log.contains("capture finished"),
        "{log}"
    );
    assert!(!log.contains("not at the default level"), "{log}");
    assert!(log.contains("ERROR") && log.contains("panic: "), "{log}");
    assert!(log.contains("the worker fell over"), "{log}");
    assert!(
        log.contains("logging_tests.rs"),
        "the panic's location: {log}"
    );
    assert!(
        !log.contains('\u{1b}'),
        "no terminal colour codes in the file"
    );
    let _ = std::fs::remove_dir_all(&home);
}
