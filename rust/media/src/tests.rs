//! Os testes de lógica de `tests/test_media.py` (HelperTests); os do barramento rodam no script headless.
use super::art::*;
use super::*;
use gio::glib::variant::ToVariant;
use std::path::{Path, PathBuf};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

fn props(pairs: &[(&str, Variant)]) -> Vec<(String, Variant)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn vardict(pairs: &[(&str, Variant)]) -> Variant {
    let d = glib::VariantDict::new(None);
    for (k, v) in pairs {
        d.insert_value(k, v);
    }
    d.end()
}

#[test]
fn pick_prefers_the_chosen_then_playing_then_paused() {
    let states = vec![State::new("a", PAUSED), State::new("b", PLAYING), State::new("c", "Stopped")];
    assert_eq!(pick(&states, None).unwrap().name, "b");
    assert_eq!(pick(&[states[0].clone(), states[2].clone()], None).unwrap().name, "a");
    assert_eq!(pick(&states, Some("a")).unwrap().name, "a");
    assert_eq!(pick(&states, Some("c")).unwrap().name, "b"); // o escolhido parou
    assert!(pick(&[states[2].clone()], None).is_none());
}

#[test]
fn times() {
    for (us, text) in [(0, "0:00"), (65_000_000, "1:05"), (3_723_000_000, "1:02:03"), (-5, "0:00")] {
        assert_eq!(fmt_time(us), text);
    }
}

#[test]
fn from_props_tolerates_missing_and_odd_values() {
    let meta = vardict(&[("xesam:artist", "Só um".to_variant()), ("mpris:length", "x".to_variant())]);
    let state = from_props("org.mpris.MediaPlayer2.firefox.instance_1_9", "", &props(&[("PlaybackStatus", "Playing".to_variant()), ("Metadata", meta)]));
    assert_eq!((state.artist.as_str(), state.length, state.title.as_str(), state.label().as_str()), ("Só um", 0, "", "firefox"));
    assert!(state.active());
}

#[test]
fn from_props_turns_wrong_types_into_empty() {
    let artists = vec!["Ok".to_variant(), 5i32.to_variant(), "".to_variant()];
    let meta = vardict(&[
        ("xesam:title", 42i32.to_variant()),
        ("xesam:album", vec!["a"].to_variant()),
        ("mpris:artUrl", 1i32.to_variant()),
        ("xesam:artist", artists.to_variant()),
        ("mpris:trackid", 9i32.to_variant()),
        ("mpris:length", true.to_variant()),
    ]);
    let state = from_props("org.mpris.MediaPlayer2.x", "", &props(&[("PlaybackStatus", 3i32.to_variant()), ("CanSeek", "sim".to_variant()), ("Metadata", meta)]));
    assert_eq!(
        (state.status.as_str(), state.title.as_str(), state.album.as_str(), state.art_url.as_str(), state.artist.as_str(), state.track_id.as_str(), state.length, state.can_seek),
        ("Stopped", "", "", "", "Ok", "", 0, false)
    );
    assert_eq!(from_props("n", "", &props(&[("Metadata", "não é dict".to_variant())])).title, "");
    // Números de outros tipos: um real trunca, um texto com inteiro vale, um negativo vira 0
    let meta = vardict(&[("mpris:length", 12.9f64.to_variant())]);
    assert_eq!(from_props("n", "", &props(&[("Metadata", meta), ("Position", "  7 ".to_variant())])).length, 12);
    assert_eq!(from_props("n", "", &props(&[("Position", (-3i64).to_variant())])).position, 0);
}

#[test]
fn labels_for_two_of_the_same_player() {
    let mut a = State::new("org.mpris.MediaPlayer2.mpv.a", PLAYING);
    a.title = "Faixa".into();
    let b = State::new("org.mpris.MediaPlayer2.mpv.b", PAUSED);
    let c = State::new("org.mpris.MediaPlayer2.mpv.c", PAUSED);
    let mut v = State::new("org.mpris.MediaPlayer2.vlc", PLAYING);
    v.identity = "VLC".into();
    assert_eq!(player_labels(&[a, b, c, v]), ["mpv: Faixa", "mpv (2)", "mpv (3)", "VLC"]);
}

#[test]
fn art_paths() {
    let cache = Path::new("/cache");
    assert_eq!(art_path("file:///tmp/capa%20um.png", cache), Some(PathBuf::from("/tmp/capa um.png")));
    let web = art_path("https://exemplo/capa.jpg", cache).unwrap();
    assert_eq!(web.parent(), Some(cache));
    assert_eq!(web.file_name().unwrap().len(), 64);
    for url in ["", "data:image/png;base64,AAAA"] {
        assert_eq!(art_path(url, cache), None, "{url}");
    }
}

fn tmpdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swaydesk-media-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn load_art_caches_and_only_accepts_images() {
    let tmp = tmpdir("load");
    let calls = std::cell::RefCell::new(Vec::new());
    let fetch = |url: &str| -> Option<Fetched> {
        calls.borrow_mut().push(url.to_string());
        let (kind, body) = if url.contains("grande") {
            ("image/png", [PNG, &vec![b'x'; ART_MAX_BYTES]].concat())
        } else if url.contains("html") {
            ("text/html", PNG.to_vec())
        } else if url.contains("ico") {
            ("image/x-icon", b"\x00\x00\x01\x00".to_vec())
        } else {
            ("image/png", PNG.to_vec())
        };
        Some(Fetched { content_type: kind.into(), body })
    };
    let yes = |_: &str| true;
    assert_eq!(load_art("https://exemplo/capa.png", &tmp, &fetch, &yes).as_deref(), Some(PNG));
    let cached = art_path("https://exemplo/capa.png", &tmp).unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
    std::fs::File::open(&cached).unwrap().set_modified(old).unwrap();
    assert_eq!(load_art("https://exemplo/capa.png", &tmp, &fetch, &yes).as_deref(), Some(PNG));
    assert_eq!(calls.borrow().len(), 1); // a segunda veio do cache
    assert!(std::fs::metadata(&cached).unwrap().modified().unwrap() > old); // e ficou marcada como recente
    for url in ["https://exemplo/grande.png", "https://exemplo/html", "https://exemplo/ico"] {
        assert!(load_art(url, &tmp, &fetch, &yes).is_none(), "{url}");
    }
    assert!(load_art("https://exemplo/outra.png", &tmp, &fetch, &|_| false).is_none()); // endereço privado
    let names: Vec<String> = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, [cached.file_name().unwrap().to_string_lossy().into_owned()]); // sem temporário
}

#[test]
fn embedded_art() {
    let b64 = |d: &[u8]| glib::base64_encode(d).to_string();
    let none = |_: &str| -> Option<Fetched> { panic!("sem rede") };
    let tmp = tmpdir("embedded");
    let url = format!("data:image/jpeg;base64,{}", b64(PNG)); // o mpv-mpris diz jpeg num PNG
    assert!(has_art(&url));
    assert_eq!(load_art(&url, &tmp, &none, &|_| false).as_deref(), Some(PNG));
    assert_eq!(load_art(&format!("data:image/png;BASE64,{}", b64(PNG)), &tmp, &none, &|_| false).as_deref(), Some(PNG));
    let huge = format!("data:image/png;base64,{}", "A".repeat(ART_MAX_BYTES * 2));
    let svg = format!("data:image/png;base64,{}", b64(b"<svg/>"));
    for bad in ["data:image/png;base64,!!!", "data:text/plain,oi", svg.as_str(), huge.as_str()] {
        assert!(load_art(bad, &tmp, &none, &|_| false).is_none(), "{}", &bad[..bad.len().min(40)]);
    }
    assert!(!has_art(""));
    assert!(!has_art("ftp://x/a.png"));
}

#[test]
fn local_art_must_be_a_small_regular_image() {
    let tmp = tmpdir("local");
    let none = |_: &str| -> Option<Fetched> { panic!("sem rede") };
    let uri = |p: &Path| format!("file://{}", p.display());
    std::fs::write(tmp.join("capa.png"), PNG).unwrap();
    assert_eq!(load_art(&uri(&tmp.join("capa.png")), &tmp, &none, &|_| false).as_deref(), Some(PNG));
    std::fs::write(tmp.join("grande.png"), [PNG, &vec![b'x'; ART_MAX_BYTES]].concat()).unwrap();
    std::fs::write(tmp.join("capa.ico"), b"\x00\x00\x01\x00").unwrap();
    let fifo = std::ffi::CString::new(tmp.join("fifo").to_str().unwrap()).unwrap();
    // SAFETY: mkfifo num caminho de teste
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    for name in ["grande.png", "capa.ico", "fifo", "nao-existe.png"] {
        assert!(load_art(&uri(&tmp.join(name)), &tmp, &none, &|_| false).is_none(), "{name}"); // o FIFO não trava
    }
}

#[test]
fn image_types_and_public_urls() {
    let kinds: Vec<Option<&str>> = [PNG, b"\xff\xd8\xff\xe0", b"GIF89a..", b"RIFF1234WEBPVP8 ", b"<svg", b""].iter().map(|d| image_type(d)).collect();
    assert_eq!(kinds, [Some("png"), Some("jpeg"), Some("gif"), Some("webp"), None, None]);
    for url in [
        "http://127.0.0.1/a.png",
        "https://localhost/a.png",
        "http://10.0.0.2/a.png",
        "http://192.168.1.1/a.png",
        "http://169.254.169.254/latest",
        "http://[::1]/a.png",
        "http://[fe80::1]/a.png",
        "http://[fd00::1]/a.png",
        "http://[::ffff:10.0.0.1]/a.png",
        "http://100.64.0.1/a.png",
        "http://0.0.0.0/a.png",
        "ftp://93.184.215.14/a.png",
        "https:///a.png",
    ] {
        assert!(!public_url(url), "{url}");
    }
    assert!(public_url("https://93.184.215.14/a.png"));
    // NAT64: vale o IPv4 de dentro
    assert!(public_url("http://[64:ff9b::5db8:d70e]/a.png"));
    assert!(!public_url("http://[64:ff9b::a00:1]/a.png"));
    // O resolvedor da conexão recusa um nome que dá num endereço local
    assert!(public_resolver("localhost:80").is_err());
    assert!(public_resolver("127.0.0.1:80").is_err());
    assert_eq!(public_resolver("93.184.215.14:443").unwrap().len(), 1);
    assert!(public_url("https://[2606:2800:21f:cb07:6820:80da:af6b:8b2c]/a.png"));
}

#[test]
fn prune_keeps_the_newest() {
    let tmp = tmpdir("prune");
    for i in 0..5u64 {
        let file = tmp.join(format!("capa{i}"));
        std::fs::write(&file, b"x").unwrap();
        std::fs::File::open(&file).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i + 1)).unwrap();
    }
    prune_art(&tmp, 2);
    let mut names: Vec<String> = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["capa3", "capa4"]);
    prune_art(Path::new("/caminho/que/nao/existe"), 2);
}

/// Na rede de verdade (cargo test -- --ignored): uma imagem pública chega pelo resolvedor conferido, e
/// uma URL local é recusada antes da conexão.
#[test]
#[ignore]
fn real_network_fetch() {
    let fetched = open_public("https://www.gnu.org/graphics/heckert_gnu.transp.small.png", &public_url).expect("imagem pública");
    assert!(fetched.content_type.starts_with("image/"));
    assert_eq!(image_type(&fetched.body), Some("png"));
    assert!(open_public("http://127.0.0.1:9/a.png", &|_| true).is_none()); // o resolvedor recusa mesmo sem o check
}
