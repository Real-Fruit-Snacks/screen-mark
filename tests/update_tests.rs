//! The updater: what it trusts, what it refuses, and how it swaps the
//! program.
//!
//! Releases here are folders on disk (`Source::Folder`), signed with keys
//! made up for the test. The "program" in them is a few lines of script
//! that answer `--version`, so a whole update runs for real (download,
//! checks, trial run, swap) without a second build of screen-mark.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ring::signature::{Ed25519KeyPair, KeyPair};
use screen_mark::update::{
    self, archive, check, install_with, keys, kind_of, manifest, Check, Error, Kind, Manifest,
    Release, Source, Version, TARGET,
};

// ---------------------------------------------------------------- helpers

/// A folder under the temp dir, deleted when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "screen-mark-update-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn source(&self) -> Source {
        Source::Folder(self.0.clone())
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A signing key and its public half.
fn key(seed: u8) -> (Ed25519KeyPair, [u8; 32]) {
    let pair = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
    let public = pair.public_key().as_ref().try_into().unwrap();
    (pair, public)
}

/// The text of a `latest.json.sig`.
fn sign(signer: &Ed25519KeyPair, manifest: &[u8]) -> String {
    format!("{}\n", manifest::to_hex(signer.sign(manifest).as_ref()))
}

fn sha256(bytes: &[u8]) -> String {
    manifest::to_hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

/// `latest.json` as the release workflow writes it
/// (.github/scripts/update-manifest.sh).
fn manifest_text(version: &str, files: &[(String, Vec<u8>)]) -> String {
    let listed: Vec<String> = files
        .iter()
        .map(|(name, bytes)| {
            format!(
                "    {{ \"name\": \"{name}\", \"size\": {}, \"sha256\": \"{}\" }}",
                bytes.len(),
                sha256(bytes)
            )
        })
        .collect();
    format!(
        "{{\n  \"name\": \"screen-mark\",\n  \"version\": \"{version}\",\n  \"files\": [\n{}\n  ]\n}}\n",
        listed.join(",\n")
    )
}

/// Put a release in `dir`: the downloads, `latest.json` and its signature.
fn publish(dir: &Dir, version: &str, files: &[(String, Vec<u8>)], signer: &Ed25519KeyPair) {
    for (name, bytes) in files {
        fs::write(dir.file(name), bytes).unwrap();
    }
    let manifest = manifest_text(version, files);
    fs::write(dir.file("latest.json"), &manifest).unwrap();
    fs::write(
        dir.file("latest.json.sig"),
        sign(signer, manifest.as_bytes()),
    )
    .unwrap();
}

fn available(dir: &Dir, running: Version, key: [u8; 32]) -> Release {
    match check(&dir.source(), running, &[key]) {
        Ok(Check::Available(release)) => release,
        other => panic!("expected a newer release, got {other:?}"),
    }
}

/// A zip, written by hand: (name, contents, deflated or stored).
fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let (mut out, mut directory) = (Vec::new(), Vec::new());
    for (name, data, deflate) in entries {
        let packed = if *deflate {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data).unwrap();
            encoder.finish().unwrap()
        } else {
            data.to_vec()
        };
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let offset = out.len() as u32;
        // What the file header and the directory entry share.
        let mut common = Vec::new();
        common.extend(20u16.to_le_bytes()); // version needed
        common.extend(0u16.to_le_bytes()); // flags
        common.extend(if *deflate { 8u16 } else { 0 }.to_le_bytes());
        common.extend([0u8; 4]); // time and date
        common.extend(crc.sum().to_le_bytes());
        common.extend((packed.len() as u32).to_le_bytes());
        common.extend((data.len() as u32).to_le_bytes());
        common.extend((name.len() as u16).to_le_bytes());

        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(&common);
        out.extend(0u16.to_le_bytes()); // extra length
        out.extend(name.as_bytes());
        out.extend(&packed);

        directory.extend(0x0201_4b50u32.to_le_bytes());
        directory.extend(20u16.to_le_bytes()); // version made by
        directory.extend(&common);
        directory.extend([0u8; 12]); // extra, comment, disk, attributes
        directory.extend(offset.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    let count = entries.len() as u16;
    out.extend(&directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disk numbers
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend((directory.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    out
}

/// A gzipped tar, written by hand.
fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, data) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(b"0000755");
        header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        tar.extend(header);
        tar.extend(*data);
        tar.resize(tar.len().next_multiple_of(512), 0);
    }
    tar.resize(tar.len() + 1024, 0);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

// A stand-in for an installed screen-mark: a script, so it can answer
// `--version` with whatever a test needs.
const PROGRAM: &str = if cfg!(windows) {
    "screen-mark.cmd"
} else {
    "screen-mark"
};
const OLD: &str = if cfg!(windows) {
    "screen-mark.old.cmd"
} else {
    "screen-mark.old"
};
const STAGED: &str = if cfg!(windows) {
    "screen-mark.new.cmd"
} else {
    "screen-mark.new"
};
const NEWER: &str = "9.9.9";
const RUNNING: Version = Version::new(2, 2, 1);

/// A program that prints `says` and exits.
fn program(says: &str) -> Vec<u8> {
    if cfg!(windows) {
        format!("@echo {says}\r\n").into_bytes()
    } else {
        format!("#!/bin/sh\necho \"{says}\"\n").into_bytes()
    }
}

/// A program that prints `says` and then fails.
fn failing_program(says: &str) -> Vec<u8> {
    if cfg!(windows) {
        format!("@echo {says}\r\n@exit /b 3\r\n").into_bytes()
    } else {
        format!("#!/bin/sh\necho \"{says}\"\nexit 3\n").into_bytes()
    }
}

fn old_program() -> Vec<u8> {
    program("screen-mark 2.2.1")
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn says(program: &Path) -> String {
    let out = std::process::Command::new(program)
        .arg("--version")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The download a replaceable copy takes, holding `program`.
fn archive_of(target: &str, program: &[u8]) -> (String, Vec<u8>) {
    if cfg!(windows) {
        let name = format!("screen-mark-v{NEWER}-{target}.zip");
        let entries: [(&str, &[u8], bool); 2] = [
            ("README.md", b"docs", true),
            ("screen-mark.exe", program, true),
        ];
        (name, zip(&entries))
    } else {
        let name = format!("screen-mark-v{NEWER}-{target}.tar.gz");
        let entries: [(&str, &[u8]); 2] = [("README.md", b"docs"), ("screen-mark", program)];
        (name, tar_gz(&entries))
    }
}

/// An installed copy and a newer release whose program is `new_program`.
struct Scene {
    release: Dir,
    install: Dir,
    exe: PathBuf,
    found: Release,
    archive: String,
}

/// `None` on a system no release is built for: nothing to install there.
fn scene(test: &str, new_program: &[u8]) -> Option<Scene> {
    let target = TARGET?;
    let (signer, public) = key(1);
    let release = Dir::new(&format!("{test}-release"));
    let (archive, bytes) = archive_of(target, new_program);
    let msi = format!("screen-mark-v{NEWER}-{target}.msi");
    publish(
        &release,
        NEWER,
        &[(archive.clone(), bytes), (msi, b"the msi".to_vec())],
        &signer,
    );
    let install = Dir::new(&format!("{test}-install"));
    let exe = install.file(PROGRAM);
    fs::write(&exe, old_program()).unwrap();
    make_executable(&exe);
    let found = available(&release, RUNNING, public);
    Some(Scene {
        release,
        install,
        exe,
        found,
        archive,
    })
}

impl Scene {
    fn replace(&self) -> Result<(), Error> {
        install_with(&self.found, &self.exe, Kind::Replace, &|_| {
            panic!("a replaceable copy runs no installer")
        })
    }
    /// The installed copy is as it was, with nothing left beside it.
    fn assert_untouched(&self) {
        assert_eq!(fs::read(&self.exe).unwrap(), old_program());
        assert!(!self.install.file(OLD).exists(), "old version left behind");
        assert!(
            !self.install.file(STAGED).exists(),
            "new version left behind"
        );
    }
}

// --------------------------------------------------------------- versions

#[test]
fn versions_are_three_numbers() {
    assert_eq!(Version::parse("2.3.0"), Some(Version::new(2, 3, 0)));
    assert_eq!(Version::parse("v2.3.0"), Some(Version::new(2, 3, 0)));
    assert_eq!(Version::parse("10.20.30"), Some(Version::new(10, 20, 30)));
    for not_one in [
        "",
        "2",
        "2.3",
        "2.3.0.1",
        "2.3.0-rc1",
        "2.3.0+build",
        "2..0",
        "a.b.c",
        " 2.3.0",
        "2.3.0 ",
        "+2.3.0",
        "2.3.-1",
        "vv2.3.0",
        "2.3.99999999999",
    ] {
        assert_eq!(Version::parse(not_one), None, "{not_one:?}");
    }
    assert_eq!(Version::new(2, 3, 0).to_string(), "2.3.0");
}

#[test]
fn versions_compare_as_numbers() {
    let v = |text| Version::parse(text).unwrap();
    assert!(v("2.10.0") > v("2.9.9"));
    assert!(v("3.0.0") > v("2.99.99"));
    assert!(v("2.2.10") > v("2.2.9"));
    assert!(v("2.2.1") == v("v2.2.1"));
}

#[test]
fn the_running_version_is_the_package_version() {
    assert_eq!(Version::running().to_string(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_setting_picks_where_releases_come_from() {
    assert_eq!(Source::from_setting(""), Source::GitHub);
    assert_eq!(Source::from_setting("  "), Source::GitHub);
    assert_eq!(
        Source::from_setting("https://intranet.example/screen-mark/downloads/"),
        Source::Web("https://intranet.example/screen-mark/downloads".into())
    );
    assert_eq!(
        Source::from_setting("http://10.0.0.5/sm"),
        Source::Web("http://10.0.0.5/sm".into())
    );
    assert_eq!(
        Source::from_setting("/srv/screen-mark"),
        Source::Folder("/srv/screen-mark".into())
    );
}

// ------------------------------------------------------------ what's trusted

#[test]
fn a_newer_signed_release_is_found_and_an_older_one_never_is() {
    let (signer, public) = key(1);
    let dir = Dir::new("newer");
    publish(&dir, "9.9.9", &[], &signer);

    let found = available(&dir, RUNNING, public);
    assert_eq!(found.version(), Version::new(9, 9, 9));

    // The same version, and a release older than what is running (a
    // replayed manifest): nothing to install.
    for running in [Version::new(9, 9, 9), Version::new(10, 0, 0)] {
        assert_eq!(
            check(&dir.source(), running, &[public]),
            Ok(Check::UpToDate {
                latest: Version::new(9, 9, 9)
            })
        );
    }
}

#[test]
fn a_release_must_be_signed_with_a_trusted_key() {
    let (signer, public) = key(1);
    let (stranger, _) = key(2);
    let dir = Dir::new("signed");
    publish(&dir, "9.9.9", &[], &signer);
    let manifest = fs::read(dir.file("latest.json")).unwrap();
    let check_it = || check(&dir.source(), RUNNING, &[public]);
    assert!(matches!(check_it(), Ok(Check::Available(_))));

    // Signed by someone else.
    fs::write(dir.file("latest.json.sig"), sign(&stranger, &manifest)).unwrap();
    assert_eq!(check_it(), Err(Error::Signature));

    // Not signed at all, or with something that isn't a signature.
    fs::remove_file(dir.file("latest.json.sig")).unwrap();
    assert_eq!(check_it(), Err(Error::Signature));
    let (zeros, letters) = ("00".repeat(64), "zz".repeat(64));
    for junk in ["", "not a signature\n", zeros.as_str(), letters.as_str()] {
        fs::write(dir.file("latest.json.sig"), junk).unwrap();
        assert_eq!(check_it(), Err(Error::Signature), "{junk:?}");
    }

    // A real signature, but the manifest was changed after it was made.
    fs::write(dir.file("latest.json.sig"), sign(&signer, &manifest)).unwrap();
    let changed = String::from_utf8(manifest.clone())
        .unwrap()
        .replace("9.9.9", "9.9.8");
    fs::write(dir.file("latest.json"), changed).unwrap();
    assert_eq!(check_it(), Err(Error::Signature));

    // Even one byte of white space.
    let mut padded = manifest.clone();
    padded.push(b'\n');
    fs::write(dir.file("latest.json"), padded).unwrap();
    assert_eq!(check_it(), Err(Error::Signature));

    // With no trusted keys nothing passes.
    fs::write(dir.file("latest.json"), &manifest).unwrap();
    assert_eq!(check(&dir.source(), RUNNING, &[]), Err(Error::Signature));
}

#[test]
fn either_trusted_key_will_do() {
    let (_, release_key) = key(1);
    let (recovery, recovery_key) = key(2);
    let dir = Dir::new("second-key");
    publish(&dir, "9.9.9", &[], &recovery);
    assert!(matches!(
        check(&dir.source(), RUNNING, &[release_key, recovery_key]),
        Ok(Check::Available(_))
    ));

    // A signature file may hold several lines; one good one is enough.
    let manifest = fs::read(dir.file("latest.json")).unwrap();
    let (stranger, _) = key(3);
    let lines = format!(
        "# signatures\n{}\n{}",
        sign(&stranger, &manifest),
        sign(&recovery, &manifest)
    );
    fs::write(dir.file("latest.json.sig"), lines).unwrap();
    assert!(matches!(
        check(&dir.source(), RUNNING, &[release_key, recovery_key]),
        Ok(Check::Available(_))
    ));
}

#[test]
fn a_signature_made_by_the_release_script_verifies() {
    // Made with `update-manifest.sh make` (OpenSSL) from the key 01 01 ...
    // 01: the script and this code agree on what is signed and how.
    let manifest = concat!(
        "{\n",
        "  \"name\": \"screen-mark\",\n",
        "  \"version\": \"9.9.9\",\n",
        "  \"files\": [\n",
        "    { \"name\": \"screen-mark-v9.9.9-windows-x64.zip\", \"size\": 21, \"sha256\": ",
        "\"07fd2e814cde9e920024e946440c48bead8be03c9b9ec8323e9aba9496ac9493\" }\n",
        "  ]\n",
        "}\n",
    );
    let signature = "c67242d95b9ac1b4852caa424c6b4ecc3644ed628aeb62699c9daef4b3ebf297\
                     62d8516622cc41cda286f01d332955439b4e1b61b553ec5985b7345328de8d00\n";
    let public =
        manifest::from_hex("8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c")
            .unwrap();
    assert_eq!(key(1).1, public);
    assert_eq!(
        manifest::verify(manifest.as_bytes(), signature.as_bytes(), &[public]),
        Ok(())
    );

    let read = Manifest::parse(manifest.as_bytes()).unwrap();
    assert_eq!(read.version, Version::new(9, 9, 9));
    assert_eq!(read.files.len(), 1);
    assert_eq!(read.files[0].name, "screen-mark-v9.9.9-windows-x64.zip");
    assert_eq!(read.files[0].size, 21);
    assert_eq!(
        manifest::to_hex(&read.files[0].sha256),
        "07fd2e814cde9e920024e946440c48bead8be03c9b9ec8323e9aba9496ac9493"
    );
}

#[test]
fn the_built_in_keys_are_two_different_keys() {
    let trusted = keys::trusted();
    assert_eq!(trusted.len(), 2);
    assert_ne!(trusted[0], trusted[1]);
    // And not the keys these tests sign with.
    for seed in 1..=3 {
        assert!(!trusted.contains(&key(seed).1));
    }
}

#[test]
fn no_manifest_is_its_own_answer() {
    let dir = Dir::new("empty");
    let (_, public) = key(1);
    match check(&dir.source(), RUNNING, &[public]) {
        Err(Error::NoManifest(place)) => assert!(place.ends_with("latest.json"), "{place}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        check(
            &Source::Folder(dir.file("no-such-folder")),
            RUNNING,
            &[public]
        )
        .map_err(|e| matches!(e, Error::NoManifest(_))),
        Err(true)
    );
}

#[test]
fn a_manifest_that_makes_no_sense_is_refused() {
    let good = r#"{"name":"screen-mark","version":"2.3.0","files":[
        {"name":"screen-mark-v2.3.0-windows-x64.zip","size":10,
         "sha256":"00000000000000000000000000000000000000000000000000000000000000ff"}]}"#;
    let parsed = Manifest::parse(good.as_bytes()).unwrap();
    assert_eq!(parsed.files[0].sha256[31], 0xff);

    // Fields this version doesn't know are fine: a later release may add some.
    let extra = good.replace("\"version\"", "\"notes\":\"hello\",\"version\"");
    assert_eq!(Manifest::parse(extra.as_bytes()), Ok(parsed));

    let refused = |text: String| match Manifest::parse(text.as_bytes()) {
        Err(Error::Manifest(why)) => why,
        other => panic!("{text}: {other:?}"),
    };
    refused("not json".into());
    refused("{}".into());
    refused(good.replace("\"screen-mark\"", "\"another-app\""));
    refused(good.replace("2.3.0\",\"files", "2.3\",\"files"));
    refused(good.replace("\"size\":10", "\"size\":-1"));
    refused(good.replace("00ff", "00f"));
    refused(good.replace("00ff", "00fg"));
    // A name is joined to a URL or a folder: it has to be only a name.
    for name in [
        "../screen-mark.zip",
        "a/b.zip",
        "a\\\\b.zip",
        ".hidden",
        "",
        "has space.zip",
        "https://elsewhere.example/x.zip",
    ] {
        let why = refused(good.replace("screen-mark-v2.3.0-windows-x64.zip", name));
        assert!(why.contains("plain file name"), "{name}: {why}");
    }
}

#[test]
fn a_release_names_its_page() {
    let (signer, public) = key(1);
    let dir = Dir::new("page");
    publish(&dir, "9.9.9", &[], &signer);
    let found = available(&dir, RUNNING, public);
    assert_eq!(found.page(), dir.0.display().to_string());
}

// --------------------------------------------------------------- archives

/// What the fixtures below hold as `screen-mark.exe` / `screen-mark`.
fn fixture_program() -> Vec<u8> {
    (1..=8)
        .map(|n| format!("stand-in for screen-mark, line {n}\n"))
        .collect::<String>()
        .into_bytes()
}

fn decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

#[test]
fn reads_a_zip_made_by_compress_archive() {
    // `Compress-Archive -Path win/* -DestinationPath windows.zip`, which is
    // how the release workflow packs the Windows zip.
    let zip = decode(concat!(
        "UEsDBBQAAAAIADoaa12Xd4NnBwAAAAUAAAAJAAAAUkVBRE1FLm1kS8lPLuYCAFBLAwQUAAAACAA6",
        "GmtdSbE66TsAAAAIAQAADwAAAHNjcmVlbi1tYXJrLmV4ZSsuScxL0c3MU0jLL1IoTi5KTc3TzU0s",
        "ytZRyMnMS1Uw5CrGr8CIkAJjQgpMCCkwJaTAjJACc0IKLLgAUEsBAhQAFAAAAAgAOhprXZd3g2cH",
        "AAAABQAAAAkAAAAAAAAAAAAAAAAAAAAAAFJFQURNRS5tZFBLAQIUABQAAAAIADoaa11JsTrpOwAA",
        "AAgBAAAPAAAAAAAAAAAAAAAAAC4AAABzY3JlZW4tbWFyay5leGVQSwUGAAAAAAIAAgB0AAAAlgAA",
        "AAAA",
    ));
    assert_eq!(
        archive::from_zip(&zip, "screen-mark.exe"),
        Ok(fixture_program())
    );
    assert_eq!(archive::from_zip(&zip, "README.md"), Ok(b"docs\n".to_vec()));
}

#[test]
fn reads_a_tar_gz_made_by_gnu_tar() {
    // `tar -czf linux.tar.gz -C linux screen-mark README.md`, as the
    // release workflow does.
    let tar_gz = decode(concat!(
        "H4sIAAAAAAAAA+3TMQ7CIBTGceaeggNY80BoXU10dPEGTVsTo9akrfcXu2hcmNoa/f8YSB5fAgRe",
        "V7Z13aTXoj2rsUiQOackE5N7GWYRM9SfnBFlvA1L1tos1I3k3ikto53ozb3ri1ZrtS/6for9vky4",
        "fVOlp0Yfb63uXp9hoS+nptYmiQRsLLCKBVws4GOBLBbIY4F1Mvc7zOWw22z3u+W1GnGPWP+L+M/+",
        "D4P+n0J1K7u//fwAAAAAAAAAAAAAAAA/4AH3bHUGACgAAA==",
    ));
    assert_eq!(
        archive::from_tar_gz(&tar_gz, "screen-mark"),
        Ok(fixture_program())
    );
    assert_eq!(
        archive::from_tar_gz(&tar_gz, "README.md"),
        Ok(b"docs\n".to_vec())
    );
}

#[test]
fn takes_one_file_out_of_a_zip() {
    let big: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
    let entries: [(&str, &[u8], bool); 4] = [
        ("README.md", b"read me", false),
        ("screen-mark.exe", &big, true),
        ("stored.bin", &big[..1000], false),
        ("empty", b"", true),
    ];
    let archive = zip(&entries);
    assert_eq!(
        archive::from_zip(&archive, "screen-mark.exe"),
        Ok(big.clone())
    );
    assert_eq!(
        archive::from_zip(&archive, "stored.bin"),
        Ok(big[..1000].to_vec())
    );
    assert_eq!(archive::from_zip(&archive, "empty"), Ok(Vec::new()));

    let missing = archive::from_zip(&archive, "screen-mark");
    assert!(matches!(missing, Err(Error::Archive(_))), "{missing:?}");

    // The sizes in front of the data may be blank (a zip written as a
    // stream): the directory's are the ones that count.
    let mut streamed = archive.clone();
    let at = find(&streamed, b"screen-mark.exe") - 30 + 14;
    streamed[at..at + 12].fill(0);
    assert_eq!(archive::from_zip(&streamed, "screen-mark.exe"), Ok(big));
}

fn find(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap()
}

#[test]
fn a_damaged_zip_is_an_error_not_a_program() {
    let data: Vec<u8> = (0..50_000u32).map(|n| (n % 7) as u8).collect();
    let entries: [(&str, &[u8], bool); 1] = [("screen-mark.exe", &data, false)];
    let archive = zip(&entries);
    assert_eq!(archive::from_zip(&archive, "screen-mark.exe"), Ok(data));

    let refused = |bytes: &[u8]| {
        let got = archive::from_zip(bytes, "screen-mark.exe");
        assert!(matches!(got, Err(Error::Archive(_))), "{got:?}");
    };
    // One byte of the program flipped: the checksum no longer matches.
    let mut flipped = archive.clone();
    flipped[1000] ^= 0x55;
    refused(&flipped);
    // Cut short, at every kind of place.
    for keep in [0, 10, 21, 100, archive.len() / 2, archive.len() - 1] {
        refused(&archive[..keep]);
    }
    refused(b"this is not a zip at all, just some text that is long enough");
}

#[test]
fn takes_one_file_out_of_a_tar_gz() {
    let big: Vec<u8> = (0..100_000u32).map(|n| (n % 13) as u8).collect();
    let entries: [(&str, &[u8]); 4] = [
        ("README.md", b"read me"),
        ("./screen-mark", &big),
        ("exact", &big[..1024]),
        ("empty", b""),
    ];
    let archive = tar_gz(&entries);
    assert_eq!(
        archive::from_tar_gz(&archive, "screen-mark"),
        Ok(big.clone())
    );
    assert_eq!(
        archive::from_tar_gz(&archive, "exact"),
        Ok(big[..1024].to_vec())
    );
    assert_eq!(archive::from_tar_gz(&archive, "empty"), Ok(Vec::new()));
    for bad in [&archive[..archive.len() / 2], b"not gzip".as_slice()] {
        let got = archive::from_tar_gz(bad, "screen-mark");
        assert!(matches!(got, Err(Error::Archive(_))), "{got:?}");
    }
    let missing = archive::from_tar_gz(&archive, "screen-mark.exe");
    assert!(matches!(missing, Err(Error::Archive(_))), "{missing:?}");
}

// ------------------------------------------------------------- installing

#[test]
fn an_update_swaps_the_program_and_keeps_the_old_one_until_the_next_start() {
    let new_program = program("screen-mark 9.9.9");
    let Some(scene) = scene("swap", &new_program) else {
        return;
    };
    assert_eq!(kind_of(&scene.exe), Kind::Replace);

    scene.replace().unwrap();

    assert_eq!(fs::read(&scene.exe).unwrap(), new_program);
    assert_eq!(says(&scene.exe), "screen-mark 9.9.9");
    // The version that was running is still there, in case the new one
    // doesn't start...
    assert_eq!(fs::read(scene.install.file(OLD)).unwrap(), old_program());
    assert!(!scene.install.file(STAGED).exists());

    // ...and goes once it has.
    update::remove_leftovers(&scene.exe);
    assert!(!scene.install.file(OLD).exists());
    assert_eq!(fs::read(&scene.exe).unwrap(), new_program);
}

#[test]
fn a_second_update_replaces_what_the_first_left() {
    let Some(scene) = scene("twice", &program("screen-mark 9.9.9")) else {
        return;
    };
    scene.replace().unwrap();
    // No restart in between: the first update's old version is replaced
    // by this one's.
    scene.replace().unwrap();
    assert_eq!(
        fs::read(scene.install.file(OLD)).unwrap(),
        program("screen-mark 9.9.9")
    );
    assert_eq!(says(&scene.exe), "screen-mark 9.9.9");
}

#[test]
fn a_download_that_is_not_what_was_signed_is_never_run() {
    let Some(scene) = scene("tampered", &program("screen-mark 9.9.9")) else {
        return;
    };
    let path = scene.release.file(&scene.archive);
    let genuine = fs::read(&path).unwrap();

    // Same size, different contents: another program in the release's place.
    let mut swapped = genuine.clone();
    let last = swapped.len() - 1;
    swapped[last] ^= 1;
    fs::write(&path, &swapped).unwrap();
    let refused = scene.replace();
    assert!(
        matches!(&refused, Err(Error::Mismatch { name, .. }) if *name == scene.archive),
        "{refused:?}"
    );
    scene.assert_untouched();

    // Shorter and longer than the release says.
    let longer = [genuine.as_slice(), b"x"].concat();
    for wrong in [&genuine[..genuine.len() - 1], longer.as_slice()] {
        fs::write(&path, wrong).unwrap();
        let refused = scene.replace();
        assert!(
            matches!(refused, Err(Error::Mismatch { .. } | Error::Fetch(_))),
            "{refused:?}"
        );
        scene.assert_untouched();
    }

    // Not there at all.
    fs::remove_file(&path).unwrap();
    let refused = scene.replace();
    assert!(matches!(refused, Err(Error::Fetch(_))), "{refused:?}");
    scene.assert_untouched();

    // Put back as it was, it installs.
    fs::write(&path, &genuine).unwrap();
    scene.replace().unwrap();
    assert_eq!(says(&scene.exe), "screen-mark 9.9.9");
}

#[test]
fn a_program_that_is_not_the_version_it_should_be_is_not_installed() {
    // The release says 9.9.9; what is in it answers 9.9.8.
    let Some(scene) = scene("wrong-version", &program("screen-mark 9.9.8")) else {
        return;
    };
    match scene.replace() {
        Err(Error::Install(why)) => assert!(why.contains("9.9.9"), "{why}"),
        other => panic!("{other:?}"),
    }
    scene.assert_untouched();
}

#[test]
fn a_program_that_does_not_run_is_not_installed() {
    // It says nothing and fails; it says the right thing and still fails.
    for (n, says) in ["", "screen-mark 9.9.9"].into_iter().enumerate() {
        let Some(scene) = scene(&format!("broken-{n}"), &failing_program(says)) else {
            return;
        };
        let refused = scene.replace();
        assert!(matches!(refused, Err(Error::Install(_))), "{refused:?}");
        scene.assert_untouched();
    }
}

#[test]
fn a_release_without_this_systems_download_is_not_installed() {
    let Some(target) = TARGET else {
        return;
    };
    let (signer, public) = key(1);
    let release = Dir::new("no-download-release");
    // A release with only another system's file.
    let other = "screen-mark-v9.9.9-plan9-x64.zip".to_string();
    publish(&release, NEWER, &[(other, b"x".to_vec())], &signer);
    let found = available(&release, RUNNING, public);
    let install = Dir::new("no-download-install");
    let exe = install.file(PROGRAM);
    fs::write(&exe, old_program()).unwrap();

    for kind in [Kind::Replace, Kind::Msi] {
        match install_with(&found, &exe, kind, &|_| panic!("nothing to run")) {
            Err(Error::NoDownload(name)) => assert!(name.contains(target), "{name}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read(&exe).unwrap(), old_program());
    }
}

#[test]
fn copies_screen_mark_does_not_own_are_left_alone() {
    let Some(scene) = scene("not-ours", &program("screen-mark 9.9.9")) else {
        return;
    };
    for kind in [Kind::Managed("Scoop"), Kind::ReadOnly, Kind::Development] {
        assert!(!kind.installs());
        let reason = kind.reason().unwrap();
        let refused = install_with(&scene.found, &scene.exe, kind, &|_| panic!("no installer"));
        assert_eq!(refused, Err(Error::Install(reason)));
        scene.assert_untouched();
    }
    assert!(Kind::Managed("Scoop").reason().unwrap().contains("Scoop"));
    assert!(Kind::Replace.installs() && Kind::Msi.installs());
}

#[test]
fn package_managers_folders_are_recognised() {
    for (path, by) in [
        (
            "C:/Users/me/scoop/apps/screen-mark/current/screen-mark.exe",
            "Scoop",
        ),
        (
            "C:/Users/me/Scoop/Apps/screen-mark/2.3.0/screen-mark.exe",
            "Scoop",
        ),
        (
            "C:/Users/me/AppData/Local/Microsoft/WinGet/Packages/RFS.screen-mark/screen-mark.exe",
            "winget",
        ),
        (
            "C:/Users/me/AppData/Local/Microsoft/WinGet/Links/screen-mark.exe",
            "winget",
        ),
        (
            "C:/ProgramData/chocolatey/lib/screen-mark/tools/screen-mark.exe",
            "Chocolatey",
        ),
    ] {
        assert_eq!(kind_of(Path::new(path)), Kind::Managed(by), "{path}");
    }
    // A folder that only sounds like one.
    let dir = Dir::new("scoop");
    let exe = dir.file(PROGRAM);
    fs::write(&exe, old_program()).unwrap();
    assert_eq!(kind_of(&exe), Kind::Replace);
}

#[test]
fn an_msi_install_is_updated_by_the_installer() {
    let new_program = program("screen-mark 9.9.9");
    let Some(scene) = scene("msi", &new_program) else {
        return;
    };
    let ran = std::cell::Cell::new(false);
    install_with(&scene.found, &scene.exe, Kind::Msi, &|msi| {
        // It is handed the release's MSI, already checked against the
        // manifest, with the running program moved out of its way.
        assert!(msi.to_string_lossy().ends_with(".msi"));
        assert_eq!(fs::read(msi).unwrap(), b"the msi");
        assert!(!scene.exe.exists());
        assert_eq!(fs::read(scene.install.file(OLD)).unwrap(), old_program());
        fs::write(&scene.exe, &new_program).unwrap();
        make_executable(&scene.exe);
        ran.set(true);
        Ok(())
    })
    .unwrap();
    assert!(ran.get());
    assert_eq!(says(&scene.exe), "screen-mark 9.9.9");
    assert_eq!(fs::read(scene.install.file(OLD)).unwrap(), old_program());
}

#[test]
fn an_installer_that_fails_leaves_the_old_version_where_it_was() {
    let Some(scene) = scene("msi-fails", &program("screen-mark 9.9.9")) else {
        return;
    };
    // It says it failed.
    let refused = install_with(&scene.found, &scene.exe, Kind::Msi, &|_| {
        Err("the installer was cancelled".to_string())
    });
    assert_eq!(
        refused,
        Err(Error::Install("the installer was cancelled".into()))
    );
    scene.assert_untouched();
    assert_eq!(says(&scene.exe), "screen-mark 2.2.1");

    // It says it worked, and installed nothing.
    let refused = install_with(&scene.found, &scene.exe, Kind::Msi, &|_| Ok(()));
    assert!(matches!(refused, Err(Error::Install(_))), "{refused:?}");
    scene.assert_untouched();

    // A changed MSI never reaches the installer.
    let msi = scene
        .release
        .file(&format!("screen-mark-v{NEWER}-{}.msi", TARGET.unwrap()));
    fs::write(&msi, b"THE MSI").unwrap();
    let refused = install_with(&scene.found, &scene.exe, Kind::Msi, &|_| {
        panic!("ran an MSI that isn't the release's")
    });
    assert!(
        matches!(refused, Err(Error::Mismatch { .. })),
        "{refused:?}"
    );
    scene.assert_untouched();
}
