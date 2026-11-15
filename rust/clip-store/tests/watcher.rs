//! O binário de verdade com um wl-paste e um cliphist falsos: os mesmos casos do tests/test_clip_store.py.
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Fakes {
    dir: PathBuf,
}

impl Fakes {
    fn new(name: &str) -> Fakes {
        let dir = std::env::temp_dir().join(format!("swaydesk-clip-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let fakes = Fakes { dir };
        // wl-paste --list-types devolve os tipos de FAKE_TYPES
        fakes.script("wl-paste", "#!/bin/sh\nprintf \"%b\" \"$FAKE_TYPES\"\n");
        // Como o cliphist de verdade, só grava depois de ler a entrada inteira
        let stored = fakes.stored();
        fakes.script("cliphist", &format!("#!/bin/sh\n[ \"$1\" = store ] && cat > \"{0}.part\" && mv \"{0}.part\" \"{0}\"\n", stored.display()));
        fakes
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.dir.join("bin").join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn stored(&self) -> PathBuf {
        self.dir.join("stored")
    }

    fn run(&self, data: &[u8], types: &str, state: &str, timeout: Option<&str>) -> i32 {
        let path = format!("{}:{}", self.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(env!("CARGO_BIN_EXE_swaydesk-clip-store"));
        command.env("PATH", path).env("FAKE_TYPES", types).env("CLIPBOARD_STATE", state).stdin(Stdio::piped());
        if let Some(seconds) = timeout {
            command.env("SWAY_CLIP_STORE_TIMEOUT", seconds);
        }
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let data = data.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
        let status = child.wait().unwrap();
        writer.join().unwrap();
        status.code().unwrap_or(-1)
    }
}

fn exists(path: &Path) -> bool {
    path.exists()
}

#[test]
fn stores_a_pdf_offered_only_as_its_type() {
    let fakes = Fakes::new("pdf");
    let data = b"%PDF-1.7\n\x00\x01binario";
    assert_eq!(fakes.run(data, "application/pdf\\n", "data", None), 0);
    assert_eq!(std::fs::read(fakes.stored()).unwrap(), data);
}

#[test]
fn leaves_text_and_images_to_their_watchers() {
    for types in ["text/plain;charset=utf-8\\napplication/x-foo\\n", "image/png\\n", "UTF8_STRING\\nSTRING\\n", "text/uri-list\\nx-special/gnome-copied-files\\n", "application/json\\n", "application/pgp-keys\\n"] {
        let fakes = Fakes::new("text");
        assert_eq!(fakes.run(&[b'x'; 100], types, "data", None), 0);
        assert!(!exists(&fakes.stored()), "{types}");
    }
}

#[test]
fn skips_sensitive_and_clear() {
    for state in ["sensitive", "clear", "nil"] {
        let fakes = Fakes::new("state");
        assert_eq!(fakes.run(b"senha", "application/octet-stream\\n", state, None), 0);
        assert!(!exists(&fakes.stored()), "{state}");
    }
}

#[test]
fn skips_items_above_the_limit_and_stores_up_to_it() {
    let fakes = Fakes::new("limit");
    assert_eq!(fakes.run(&vec![0u8; 5_000_001], "application/octet-stream\\n", "data", None), 0);
    assert!(!exists(&fakes.stored()));
    let data: Vec<u8> = (0..5_000_000u32).map(|i| (i % 256) as u8).collect();
    assert_eq!(fakes.run(&data, "application/octet-stream\\n", "data", None), 0);
    assert_eq!(std::fs::read(fakes.stored()).unwrap(), data);
}

#[test]
fn a_stuck_cliphist_does_not_hold_the_watcher() {
    let fakes = Fakes::new("stuck");
    fakes.script("cliphist", "#!/bin/sh\nexec sleep 60\n"); // um processo só, como o cliphist
    let start = Instant::now();
    assert_eq!(fakes.run(&[b'x'; 200_000], "application/pdf\\n", "data", Some("1")), 1);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn empty_input_is_not_stored() {
    let fakes = Fakes::new("empty");
    assert_eq!(fakes.run(b"", "application/octet-stream\\n", "data", None), 0);
    assert!(!exists(&fakes.stored()));
}
