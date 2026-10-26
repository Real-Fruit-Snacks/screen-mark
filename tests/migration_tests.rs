//! 2.0 renamed the tool from `hydroshot` to `screen-mark`. These tests prove a
//! 1.x user's config and history come across on first run, and that a fresh
//! install is not touched by migration code.
//!
//! They point `dirs` at a temp directory through XDG_* and so run on Linux
//! only; the migration code itself is platform-independent.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// `dirs` reads the environment per call, and env vars are process-wide, so
/// every test here takes this lock and restores the variables when done.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct TempHome {
    root: PathBuf,
    saved: Vec<(&'static str, Option<String>)>,
}

impl TempHome {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("screen-mark-mig-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        let mut saved = Vec::new();
        for (k, v) in [
            ("XDG_CONFIG_HOME", root.join("config")),
            ("XDG_DATA_HOME", root.join("data")),
        ] {
            saved.push((k, std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        Self { root, saved }
    }
    fn config(&self) -> PathBuf {
        self.root.join("config")
    }
    fn data(&self) -> PathBuf {
        self.root.join("data")
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn config_is_copied_from_hydroshot_on_first_run() {
    let _l = ENV_LOCK.lock().unwrap();
    let home = TempHome::new("config");
    let old = home.config().join("hydroshot");
    fs::create_dir_all(&old).unwrap();
    fs::write(
        old.join("config.toml"),
        "[general]\ndefault_color = \"mauve\"\ndefault_thickness = 7.0\n",
    )
    .unwrap();

    let cfg = screen_mark::config::Config::load();

    assert_eq!(cfg.general.default_color, "mauve");
    assert_eq!(cfg.general.default_thickness, 7.0);
    let new = home.config().join("screen-mark").join("config.toml");
    assert!(new.is_file(), "config was copied to the new location");
    assert!(
        old.join("config.toml").is_file(),
        "the 1.x file is left for 1.x"
    );
    assert_eq!(
        screen_mark::config::Config::config_path().unwrap(),
        new,
        "config_path points at screen-mark/"
    );
}

#[test]
fn existing_screen_mark_config_wins_over_hydroshot() {
    let _l = ENV_LOCK.lock().unwrap();
    let home = TempHome::new("config-both");
    let old = home.config().join("hydroshot");
    let new = home.config().join("screen-mark");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        old.join("config.toml"),
        "[general]\ndefault_color = \"mauve\"\n",
    )
    .unwrap();
    fs::write(
        new.join("config.toml"),
        "[general]\ndefault_color = \"green\"\n",
    )
    .unwrap();

    let cfg = screen_mark::config::Config::load();
    assert_eq!(cfg.general.default_color, "green");
}

#[test]
fn fresh_install_writes_defaults_and_nothing_else() {
    let _l = ENV_LOCK.lock().unwrap();
    let home = TempHome::new("fresh");

    let cfg = screen_mark::config::Config::load();
    assert_eq!(cfg.general.default_color, "red");
    assert!(home
        .config()
        .join("screen-mark")
        .join("config.toml")
        .is_file());
    assert!(
        !home.config().join("hydroshot").exists(),
        "no 1.x dir was created"
    );
}

#[test]
fn history_moves_from_both_hydroshot_locations() {
    let _l = ENV_LOCK.lock().unwrap();
    let home = TempHome::new("history");
    // pre-0.6: roaming config dir. 1.x: local data dir.
    let pre06 = home.config().join("hydroshot").join("history");
    let onex = home.data().join("hydroshot").join("history");
    fs::create_dir_all(&pre06).unwrap();
    fs::create_dir_all(&onex).unwrap();
    fs::write(pre06.join("1000.png"), b"a").unwrap();
    fs::write(onex.join("2000.png"), b"b").unwrap();
    fs::write(onex.join("notes.txt"), b"not a capture").unwrap();

    // Any history call ensures the dir and runs the migration.
    let listed = screen_mark::history::list_history();

    let new = home.data().join("screen-mark").join("history");
    assert_eq!(screen_mark::history::history_dir().unwrap(), new);
    assert!(new.join("1000.png").is_file(), "pre-0.6 capture moved");
    assert!(new.join("2000.png").is_file(), "1.x capture moved");
    assert!(!pre06.exists(), "emptied legacy dir is removed");
    assert!(
        onex.join("notes.txt").is_file(),
        "non-PNG files are left alone"
    );
    assert!(onex.exists(), "legacy dir with leftovers is kept");
    assert_eq!(listed.len(), 2);
}
