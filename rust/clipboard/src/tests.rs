//! Os testes de lógica do clipboard: test_cliphist_parse.py, test_search.py, test_recents.py,
//! test_thumbs.py, test_pin_store.py e test_confirm.py.
use super::*;
use crate::pins::PIN_VERSION;
use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swaydesk-clip-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// --- parse ---
#[test]
fn parse_lines() {
    assert_eq!(parse_line("42\t  hello\tworld  "), Some(Entry::new("42", "  hello\tworld  ", TEXT, None)));
    assert_eq!(parse_line("42\t").unwrap().preview, "");
    assert_eq!(parse_line("1\ta\tb\tc").unwrap().preview, "a\tb\tc");
    assert_eq!(parse_line("7\tback\\slash \\t \\n \"quote\"").unwrap().preview, "back\\slash \\t \\n \"quote\"");
    assert_eq!(parse_line("42\tação café AÇÃO 😀").unwrap().preview, "ação café AÇÃO 😀");
    for invalid in ["", "abc", "-1", "1x", " 1", "1 ", "+1"] {
        assert!(parse_line(&format!("{invalid}\ttexto")).is_none(), "{invalid}");
    }
}

#[test]
fn parse_lists() {
    let entries = parse_list(b"\n1\tok\n2 sem tab\n\tid vazio\n3\t\n4\tbinary data image/png\n");
    assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["1", "3", "4"]);
    assert_eq!((entries[2].kind.as_str(), entries[2].mime.as_deref()), (IMAGE, Some("image/png")));
    let entries = parse_list(b"1\tcaf\xe9\r\n2\tbom\r\n");
    assert_eq!(entries[0].preview, "caf\u{fffd}");
    assert_eq!(entries[1].preview, "bom");
}

#[test]
fn cliphist_05_previews() {
    for (preview, mime) in [("[[ binary data 78 KiB png 300x200 ]]", "image/png"), ("[[ binary data 2 MiB jpeg 4000x3000 ]]", "image/jpeg"), ("[[ binary data 512 B gif 1x1 ]]", "image/gif")] {
        let e = parse_line(&format!("9\t{preview}")).unwrap();
        assert_eq!((e.kind.as_str(), e.mime.as_deref()), (IMAGE, Some(mime)));
    }
    for preview in ["[[ binary data ]]", "[[ binary data 12 KiB ]]", "veja [[ binary data 78 KiB png 300x200 ]]", "[[ binary data 78 KiB png 300x200 ]] e mais"] {
        assert_eq!(parse_line(&format!("9\t{preview}")).unwrap().kind, TEXT, "{preview}");
    }
}

#[test]
fn a_pin_saved_as_text_from_cliphist_05_is_read_as_an_image() {
    let mut raw = serde_json::json!({"sha": "a".repeat(64), "kind": "texto", "mime": null, "source_id": "9",
                                     "preview": "[[ binary data 78 KiB png 300x200 ]]", "pinned_at": 1.0, "size": 79452});
    let pin = Pin::from_json(&raw).unwrap();
    assert_eq!((pin.kind.as_str(), pin.mime.as_deref()), (IMAGE, Some("image/png")));
    raw["preview"] = "um texto comum".into();
    assert_eq!(Pin::from_json(&raw).unwrap().kind, TEXT);
}

#[test]
fn preview_without_zero_bytes_for_display_and_search() {
    let preview: String = "https://exemplo.com".chars().flat_map(|c| [c, '\0']).collect();
    let e = parse_line(&format!("9\t{preview}")).unwrap();
    assert_eq!(e.kind, TEXT);
    assert_eq!(display_preview(&e.preview), "https://exemplo.com");
    assert!(matches("exemplo", &search_fields(&e, None)));
}

// --- busca ---
#[test]
fn search() {
    assert_eq!(normalize("AÇÃO"), "acao");
    assert_eq!(normalize("ß"), "ss");
    assert!(matches("café", &["cafe"]));
    assert!(matches("acao", &["AÇÃO"]));
    assert!(matches("foo bar", &["foo bar baz"]) && !matches("foo qux", &["foo bar baz"]));
    assert!(matches("foo bar", &["bar foo"]) && matches("", &["qualquer coisa"]));
    let image = Entry::new("1", "binary data image/png", IMAGE, Some("image/png"));
    for q in ["imagem", "png", "image/png"] {
        assert!(matches(q, &search_fields(&image, None)), "{q}");
    }
    assert!(!matches("imagem", &search_fields(&Entry::new("1", "hello", TEXT, None), None)));
    let full = format!("{} segredo no fim", "x".repeat(120));
    assert!(matches("segredo", &search_fields(&Entry::new("1", &"x".repeat(100), TEXT, None), Some(&full))));
    let text = haystack(&["AÇÃO", "", "Café"]);
    assert!(matches_terms(&terms("acao cafe"), &text) && !matches_terms(&terms("acao cha"), &text));
}

// --- recentes ---
fn sample_pin() -> Pin {
    Pin { sha: "a".repeat(64), kind: TEXT.into(), mime: None, preview: "hello".into(), source_id: "5".into(), pinned_at: 1.0, size: 5 }
}

#[test]
fn recents() {
    let item = Entry::new("5", "hello", TEXT, None);
    let pins = [sample_pin()];
    let a = "a".repeat(64);
    assert!(!recent_visible(&item, &pins, Some(&a)));
    assert!(recent_visible(&item, &pins, Some(&"b".repeat(64))));
    let again = Entry::new("9", "hello", TEXT, None);
    assert!(!recent_visible(&again, &pins, Some(&a)));
    assert!(recent_visible(&again, &pins, None));
    assert!(!recent_visible(&item, &pins, None));
}

// --- miniaturas ---
fn set_mtime(path: &Path, secs: u64) {
    std::fs::File::open(path).unwrap().set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
}

#[test]
fn thumbs_prune_keeps_the_newest() {
    let root = tmp("thumbs");
    for i in 0..520u64 {
        let p = root.join(format!("{i:064x}.png"));
        std::fs::write(&p, b"x").unwrap();
        set_mtime(&p, i + 1);
    }
    assert_eq!(prune_thumbs(&root, 500, &HashSet::new()), 20);
    assert!(root.join(format!("{:064x}.png", 20)).exists() && !root.join(format!("{:064x}.png", 19)).exists());
    let (pinned, wiped) = ("a".repeat(64), "b".repeat(64));
    let root = tmp("thumbs0");
    for sha in [&pinned, &wiped] {
        std::fs::write(root.join(format!("{sha}.png")), b"x").unwrap();
    }
    let temp = root.join(format!(".{}.png.abc123", "a".repeat(64)));
    std::fs::write(&temp, b"x").unwrap();
    assert_eq!(prune_thumbs(&root, 0, &HashSet::from([pinned.clone()])), 1);
    assert!(root.join(format!("{pinned}.png")).exists() && !temp.exists());
}

#[test]
fn thumb_paths_and_scale() {
    let root = Path::new("/r");
    assert_eq!(thumb_path(root, &"a".repeat(64)).unwrap(), root.join(format!("{}.png", "a".repeat(64))));
    for invalid in ["../x", "ABC", &"a".repeat(63)] {
        assert!(thumb_path(root, invalid).is_err());
    }
    assert_eq!(scale_box(1920, 1080, 40), (40, 22));
    assert_eq!(scale_box(100, 300, 40), (13, 40));
    assert_eq!(scale_box(1, 1, 40), (1, 1));
    remove_thumb(&tmp("rm"), &"a".repeat(64)); // sem o arquivo: nada
}

// --- fixados ---
fn store(name: &str) -> PinStore {
    PinStore::new(&tmp(name).join("data"))
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn pins_permissions_and_dedupe() {
    let mut s = store("perm");
    let item = Entry::new("10", "hello", TEXT, None);
    let pin = s.pin(&item, b"hello", Some(1.0)).unwrap();
    assert_eq!((mode(&s.root), mode(&s.pins_dir), mode(&s.blob_path(&pin.sha).unwrap()), mode(&s.meta_path)), (0o700, 0o700, 0o600, 0o600));
    s.pin(&item, b"hello", Some(2.0)).unwrap();
    let pins = s.load();
    assert_eq!(pins.len(), 1);
    assert_eq!(std::fs::read_dir(&s.pins_dir).unwrap().count(), 1);
    // O mesmo conteúdo de novo sobe para o topo com a data nova
    let one = s.pin(&item, b"one", Some(3.0)).unwrap();
    s.pin(&Entry::new("11", "two", TEXT, None), b"two", Some(4.0)).unwrap();
    s.pin(&item, b"one", Some(5.0)).unwrap();
    let pins = s.load();
    assert_eq!((pins[0].sha.as_str(), pins[0].pinned_at), (one.sha.as_str(), 5.0));
    assert_eq!(pins.len(), 3);
}

#[test]
fn pins_load_is_careful() {
    let mut s = store("load");
    s.ensure().unwrap();
    let orphan = s.pins_dir.join(format!("{}.bin", "d".repeat(64)));
    std::fs::write(&orphan, b"guardado").unwrap();
    std::fs::write(&s.meta_path, "{").unwrap();
    assert!(s.load().is_empty());
    assert!(orphan.exists()); // sem leitura completa, nenhum órfão sai
    assert_eq!(std::fs::read_to_string(&s.meta_path).unwrap(), "{");
    for raw in [
        serde_json::json!({"version": 1, "items": [{"sha": "a".repeat(64)}, {"preview": "x"}]}),
        serde_json::json!({"version": 1, "items": [{"sha": "../../etc/passwd", "kind": "texto", "mime": null, "preview": "x", "source_id": "1", "pinned_at": 1, "size": 1}]}),
        serde_json::json!({"version": 1, "items": [{"sha": "c".repeat(64), "kind": "texto", "mime": null, "preview": "x", "source_id": "1", "pinned_at": 1, "size": 1}]}),
    ] {
        std::fs::write(&s.meta_path, raw.to_string()).unwrap();
        assert!(s.load().is_empty());
    }
}

#[test]
fn pins_sha_mismatch_orphans_and_unpin() {
    let mut s = store("sha");
    let pin = s.pin(&Entry::new("10", "hello", TEXT, None), b"hello", None).unwrap();
    let orphan = s.pins_dir.join(format!("{}.bin", "b".repeat(64)));
    std::fs::write(&orphan, b"orphan").unwrap();
    assert_eq!(s.load().iter().map(|p| p.sha.as_str()).collect::<Vec<_>>(), [pin.sha.as_str()]);
    assert!(!orphan.exists());
    let second = s.pin(&Entry::new("11", "two", TEXT, None), b"two", None).unwrap();
    assert!(s.unpin(&pin.sha).unwrap());
    assert_eq!(s.load().iter().map(|p| p.sha.as_str()).collect::<Vec<_>>(), [second.sha.as_str()]);
    assert!(!s.blob_path(&pin.sha).unwrap().exists());
    std::fs::write(s.blob_path(&second.sha).unwrap(), b"tampered").unwrap();
    assert!(s.load().is_empty());
    assert!(!s.blob_path(&second.sha).unwrap().exists());
}

#[test]
fn incomplete_metadata_keeps_blobs_and_refuses_to_save() {
    let mut s = store("incomplete");
    let pin = s.pin(&Entry::new("10", "hello", TEXT, None), b"hello", None).unwrap();
    let blob = s.blob_path(&pin.sha).unwrap();
    for text in [
        "{}".to_string(),
        serde_json::json!({"version": PIN_VERSION + 1, "items": [pin.to_json()]}).to_string(),
        serde_json::json!({"version": PIN_VERSION, "items": [pin.to_json(), {"sha": "x"}]}).to_string(),
    ] {
        std::fs::write(&s.meta_path, &text).unwrap();
        s.load();
        assert!(blob.exists(), "{text}");
        assert!(s.pin(&Entry::new("11", "two", TEXT, None), b"two", None).is_err(), "{text}");
        assert_eq!(std::fs::read_to_string(&s.meta_path).unwrap(), text);
    }
}

#[test]
fn python_pins_json_is_read() {
    // O formato que o Python grava (json.dumps com indent=2), com a versão como inteiro e um real
    let mut s = store("python");
    s.ensure().unwrap();
    std::fs::write(s.blob_path(&sha256(b"oi")).unwrap(), b"oi").unwrap();
    let text = format!("{{\n  \"version\": 1.0,\n  \"items\": [\n    {{\n      \"sha\": \"{}\",\n      \"kind\": \"texto\",\n      \"mime\": null,\n      \"preview\": \"oi\",\n      \"source_id\": \"3\",\n      \"pinned_at\": 17.5,\n      \"size\": 2\n    }}\n  ]\n}}", sha256(b"oi"));
    std::fs::write(&s.meta_path, text).unwrap();
    let pins = s.load();
    assert_eq!((pins.len(), s.last_read_ok, pins[0].pinned_at), (1, true, 17.5));
}

// --- confirmação ---
#[test]
fn confirm_needs_a_second_click_in_time() {
    let mut c = Confirm::new(Duration::from_secs(3));
    let t = Instant::now();
    assert!(!c.click(t));
    assert!(c.click(t + Duration::from_secs(2)));
    assert!(!c.click(t + Duration::from_secs(10)));
    assert!(!c.click(t + Duration::from_secs(14))); // passou do prazo: arma de novo
}

// --- o programa de verdade, por um cliphist e um wl-copy de mentira ---
#[test]
fn tools_talk_to_the_programs() {
    let dir = tmp("tools");
    let cliphist = dir.join("cliphist");
    std::fs::write(&cliphist, "#!/bin/sh\ncase $1 in\n  list) printf '2\\tdois\\n1\\tum\\n' ;;\n  decode) read -r line; printf 'conteudo-%s' \"${line%%\t*}\" ;;\n  delete) cat > \"$0.deleted\" ;;\n  wipe) exit 3 ;;\nesac\n").unwrap();
    let wl_copy = dir.join("wl-copy");
    std::fs::write(&wl_copy, "#!/bin/sh\necho \"$@\" > \"$0.args\"; cat > \"$0.data\"; [ \"$2\" = bad/x ] && { echo quebrou >&2; exit 1; }; exit 0\n").unwrap();
    for p in [&cliphist, &wl_copy] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tools = Tools { cliphist: cliphist.clone(), wl_copy: wl_copy.clone() };
    let (entries, error) = tools.list_entries();
    assert_eq!((entries.len(), error), (2, None));
    assert_eq!(tools.decode("2").unwrap(), b"conteudo-2");
    tools.delete("7").unwrap();
    assert_eq!(std::fs::read(dir.join("cliphist.deleted")).unwrap(), b"7\t\n");
    assert!(tools.wipe().is_err());
    tools.copy(b"abc", Some("text/plain")).unwrap();
    assert_eq!(std::fs::read(dir.join("wl-copy.data")).unwrap(), b"abc");
    assert_eq!(std::fs::read_to_string(dir.join("wl-copy.args")).unwrap().trim(), "--type text/plain");
    assert_eq!(tools.copy(b"abc", Some("bad/x")).unwrap_err().0, "quebrou");
    assert!(tools.copy(b"", None).is_err());
    let missing = Tools { cliphist: dir.join("nao-existe"), ..tools };
    assert!(missing.list_entries().1.unwrap().contains("No such file or directory"));
}
