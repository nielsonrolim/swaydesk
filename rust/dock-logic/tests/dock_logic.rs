//! Os mesmos casos do `tests/test_dock.py` para a lógica sem GTK.
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde_json::json;
use swaydesk_dock_logic::*;

fn tmpdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dock-logic-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn none(_: &str) -> Option<String> {
    None
}

#[test]
fn config_defaults_and_values() {
    let (config, warnings) = parse_config(&table(""));
    assert_eq!(config, Config::default());
    assert!(warnings.is_empty());
    let (config, warnings) = parse_config(&table(
        "mode = \"always\"\nicon_size = 48\nhide_delay_ms = 0\nanimation_ms = 1000\nlauncher = \"x\"\nignore = [\"a\"]\noutputs = [\"HDMI-A-1\"]\n",
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.mode, "always");
    assert_eq!((config.icon_size, config.hide_delay_ms, config.animation_ms), (48, 0, 1000));
    assert_eq!(config.launcher, "x");
    assert_eq!(config.ignore, s(&["a"]));
    assert_eq!(config.outputs, s(&["HDMI-A-1"]));
}

#[test]
fn launcher_default_is_the_app_grid_and_empty_hides_the_button() {
    // "sway-drawer" vira o componente da grade (a dock o resolve); vazio é a escolha de não ter o botão
    assert_eq!(Config::default().launcher, "sway-drawer");
    let (config, warnings) = parse_config(&table("launcher = \"\"\n"));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.launcher, "");
}

#[test]
fn bad_config_values_keep_the_default() {
    let (config, warnings) = parse_config(&table(
        "mode = \"x\"\nicon_size = 8\nhide_delay_ms = true\nanimation_ms = 2000\nlauncher = 1\nignore = [1]\noutputs = \"a\"\nfoo = 1\n",
    ));
    assert_eq!(config, Config::default());
    assert_eq!(
        warnings,
        s(&[
            "config: invalid value in mode; using the default",
            "config: invalid value in icon_size; using the default",
            "config: invalid value in hide_delay_ms; using the default",
            "config: invalid value in animation_ms; using the default",
            "config: invalid value in launcher; using the default",
            "config: invalid value in ignore; using the default",
            "config: invalid value in outputs; using the default",
            "config: unknown option: foo",
        ])
    );
}

#[test]
fn load_config_missing_and_broken() {
    let dir = tmpdir("config");
    assert_eq!(load_config(&dir.join("nada.toml")), (Config::default(), vec![]));
    let file = dir.join("dock.toml");
    fs::write(&file, "mode = ").unwrap();
    let (config, warnings) = load_config(&file);
    assert_eq!(config, Config::default());
    assert!(warnings[0].starts_with("config: could not read"));
}

fn tree() -> serde_json::Value {
    json!({"type": "root", "nodes": [
        {"type": "output", "name": "__i3", "nodes": [
            {"type": "workspace", "name": "__i3_scratch", "nodes": [], "floating_nodes": [
                {"type": "floating_con", "id": 9, "pid": 1, "app_id": "escondido", "nodes": [], "floating_nodes": []}]}]},
        {"type": "output", "name": "eDP-1", "current_workspace": "1",
         "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080}, "nodes": [
            {"type": "workspace", "name": "1", "nodes": [
                {"type": "con", "id": 5, "pid": 10, "app_id": "kitty", "name": "zsh", "focused": true,
                 "rect": {"x": 0, "y": 0, "width": 960, "height": 1080}, "nodes": [], "floating_nodes": []},
                {"type": "con", "id": 3, "pid": 11, "app_id": null, "name": "Firefox",
                 "window_properties": {"class": "firefox"},
                 "rect": {"x": 960, "y": 0, "width": 960, "height": 1080}, "nodes": [], "floating_nodes": []}],
             "floating_nodes": [
                {"type": "floating_con", "id": 7, "pid": 12, "app_id": "pavucontrol", "urgent": true,
                 "rect": {"x": 100, "y": 100, "width": 300, "height": 200}, "nodes": [], "floating_nodes": []}]},
            {"type": "workspace", "name": "2", "nodes": [
                {"type": "con", "id": 8, "pid": 0, "app_id": "sem-pid", "nodes": [], "floating_nodes": []}],
             "floating_nodes": []}]}]})
}

#[test]
fn windows_skip_the_scratchpad_and_follow_creation_order() {
    let windows = windows_from_tree(&tree());
    let ids: Vec<i64> = windows.iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![3, 5, 7]);
    assert_eq!(windows[0].app_id, "firefox"); // a classe do Xwayland
    assert!(windows[1].focused);
    assert!(windows[2].urgent && windows[2].floating);
    assert_eq!(windows[1].output, "eDP-1");
    assert_eq!(windows[1].workspace, "1");
    assert_eq!(windows[2].rect, (100, 100, 300, 200));
}

#[test]
fn outputs_and_geometry() {
    let outputs = outputs_from_tree(&tree());
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs["eDP-1"], ((0, 0, 1920, 1080), Some("1".to_string())));
    assert_eq!(output_at(&outputs, (0, 0, 1920, 1080)), Some("eDP-1".to_string()));
    assert_eq!(output_at(&outputs, (0, 0, 1280, 720)), None);
}

#[test]
fn overlap_and_show() {
    let windows = windows_from_tree(&tree());
    let dock = dock_rect((0, 0, 1920, 1080), 400, 60);
    assert_eq!(dock, (760, 1020, 400, 60));
    assert!(overlapped(dock, &windows, "eDP-1", "1"));
    assert!(!overlapped(dock, &windows, "eDP-1", "2"));
    assert!(!rects_overlap((0, 0, 10, 10), (10, 0, 10, 10)));
    assert!(should_show("always", false, false, false, true));
    assert!(should_show("auto-hide", true, false, false, true));
    assert!(!should_show("auto-hide", false, false, false, false));
    assert!(should_show("overlap", false, false, false, false));
    assert!(!should_show("overlap", false, false, false, true));
    assert!(should_show("overlap", false, true, false, true));
    assert!(should_show("overlap", false, false, true, true));
}

#[test]
fn polling_only_for_visible_floating_windows_in_overlap() {
    let windows = windows_from_tree(&tree());
    let mut outputs = outputs_from_tree(&tree());
    assert!(needs_polling("overlap", &windows, &outputs));
    assert!(!needs_polling("auto-hide", &windows, &outputs));
    outputs.insert("eDP-1".into(), ((0, 0, 1920, 1080), Some("2".into())));
    assert!(!needs_polling("overlap", &windows, &outputs));
    assert!(uses_hotspot("overlap") && uses_hotspot("auto-hide") && !uses_hotspot("always"));
}

#[test]
fn stuck_states() {
    assert_eq!(stuck_state(None, false, None, false), None);
    assert_eq!(stuck_state(Some(500), false, None, false), None);
    assert_eq!(stuck_state(Some(1000), false, None, false), Some(Stuck::Menu));
    assert_eq!(stuck_state(Some(5000), true, None, false), None);
    assert_eq!(stuck_state(None, false, Some(10), false), Some(Stuck::Drag));
    assert_eq!(stuck_state(None, false, Some(100), true), None);
    assert_eq!(stuck_state(None, false, Some(5000), true), Some(Stuck::Drag));
}

#[test]
fn slide_eases_and_reverses_from_where_it_is() {
    let mut slide = Slide::default();
    assert!(!slide.start(0.0, 0));
    assert!(slide.start(1.0, 0));
    assert!(slide.step(100, 200));
    assert!((slide.progress - ease_out(0.5)).abs() < 1e-9);
    let at = slide.progress;
    assert!(slide.start(0.0, 100)); // volta de onde parou
    assert_eq!(slide.origin, at);
    // A volta tem a distância percorrida: termina em 200 * at ms
    assert!(!slide.step(100 + (200.0 * at).ceil() as i64, 200));
    assert_eq!(slide.progress, 0.0);
    assert!(!slide.running);
    assert_eq!(ease_out(0.0), 0.0);
    assert_eq!(ease_out(1.0), 1.0);
}

fn entries() -> Vec<AppEntry> {
    vec![
        AppEntry { id: "org.mozilla.firefox".into(), wm_class: "".into() },
        AppEntry { id: "code".into(), wm_class: "Code".into() },
        AppEntry { id: "nemo".into(), wm_class: "".into() },
        AppEntry { id: "sway-theme-app".into(), wm_class: "".into() },
    ]
}

#[test]
fn match_app_order() {
    let e = entries();
    assert_eq!(match_app("org.mozilla.firefox", &e), Some("org.mozilla.firefox".into()));
    assert_eq!(match_app("Code", &e), Some("code".into()));
    assert_eq!(match_app("firefox", &e), Some("org.mozilla.firefox".into()));
    assert_eq!(match_app("sway-theme-app.py", &e), Some("sway-theme-app".into()));
    assert_eq!(match_app("", &e), None);
    assert_eq!(match_app("gimp", &e), None);
}

#[test]
fn items_pinned_first_then_open_in_order() {
    let e = entries();
    let resolve = |id: &str| match_app(id, &e);
    let windows = windows_from_tree(&tree());
    let items = build_items(&s(&["nemo", "firefox", "nemo"]), &windows, &resolve, &s(&["PAVUCONTROL"]));
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, vec!["nemo", "org.mozilla.firefox", "kitty"]);
    assert!(items[0].pinned && items[0].windows.is_empty());
    assert!(items[1].pinned && items[1].windows.len() == 1);
    assert!(!items[2].pinned && items[2].focused());
    assert!(!items[1].urgent());
}

#[test]
fn a_pin_without_an_installed_app_is_hidden() {
    // "thunar" não tem .desktop nas entradas: o fixado some, e os outros ficam na ordem
    let e = entries();
    let resolve = |id: &str| match_app(id, &e);
    let items = build_items(&s(&["thunar", "firefox", "code"]), &[], &resolve, &[]);
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, vec!["org.mozilla.firefox", "code"]);
    // Com uma janela aberta daquele app_id (um app sem .desktop), ele aparece fixado, com a janela
    let windows = windows_from_tree(&tree());
    let items = build_items(&s(&["kitty", "firefox"]), &windows, &resolve, &[]);
    assert_eq!(items[0].key, "kitty");
    assert!(items[0].pinned && items[0].windows.len() == 1);
}

#[test]
fn pins_load_save_and_set_aside() {
    let dir = tmpdir("pins");
    let file = dir.join("sway-dock/pinned");
    assert_eq!(load_pins(&file).unwrap(), Vec::<String>::new());
    save_pins(&file, &s(&["a", "b"])).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "a\nb\n");
    fs::write(&file, " a \n\nb\na\n").unwrap();
    assert_eq!(load_pins(&file).unwrap(), s(&["a", "b"]));
    let first = set_aside(&file, 100).unwrap();
    assert_eq!(first.file_name().unwrap(), "pinned.bad-100");
    fs::write(&file, "x").unwrap();
    let second = set_aside(&file, 100).unwrap();
    assert_eq!(second.file_name().unwrap(), "pinned.bad-100-1");
    assert!(first.exists() && second.exists() && !file.exists());
}

#[test]
fn load_pins_other_errors_are_raised() {
    let dir = tmpdir("pins-err");
    // Uma pasta no lugar do arquivo: tratar como vazio apagaria os fixados ao salvar
    assert!(load_pins(&dir).is_err());
}

#[test]
fn pin_requests_compare_like_the_dock() {
    let e = entries();
    let resolve = |id: &str| match_app(id, &e);
    let pins = s(&["firefox", "nemo"]);
    assert!(is_pinned(&pins, "org.mozilla.firefox", &resolve));
    assert_eq!(apply_pin_request(&pins, "pin", "org.mozilla.firefox", &resolve), pins);
    assert_eq!(apply_pin_request(&pins, "pin", "code", &resolve), s(&["firefox", "nemo", "code"]));
    assert_eq!(apply_pin_request(&s(&["firefox", "org.mozilla.firefox", "nemo"]), "unpin", "firefox", &resolve), s(&["nemo"]));
    assert_eq!(apply_pin_request(&pins, "pin", "gimp", &none), s(&["firefox", "nemo", "gimp"]));
}

#[test]
fn request_queue() {
    let dir = tmpdir("queue");
    let queue = dir.join("sway-dock.requests");
    assert_eq!(take_requests(&queue).unwrap(), vec![]);
    queue_request(&queue, "pin", "firefox").unwrap();
    queue_request(&queue, "unpin", "nemo").unwrap();
    let mut extra = fs::read(&queue).unwrap();
    extra.extend_from_slice(b"bogus\tx\npin\t   \npin\t\xff\xfe\n");
    fs::write(&queue, extra).unwrap();
    let taken = take_requests(&queue).unwrap();
    assert_eq!(&taken[..2], &[("pin".to_string(), "firefox".to_string()), ("unpin".to_string(), "nemo".to_string())]);
    assert_eq!(taken.len(), 3);
    assert!(taken[2].1.contains('\u{fffd}'));
    assert_eq!(fs::read(&queue).unwrap(), b"");
}

#[test]
fn pin_command_without_the_dock_queues_nothing() {
    let dir = tmpdir("cmd");
    let queue = dir.join("sway-dock.requests");
    assert!(!pin_command("pin", "firefox", &queue, &dir).unwrap());
    assert!(!queue.exists());
}

#[test]
fn pinned_ids_for_the_grid() {
    let dir = tmpdir("ids");
    assert_eq!(read_pinned_ids(&dir).unwrap(), None);
    write_pinned_ids(&dir, &s(&["firefox", "nemo"])).unwrap();
    let ids = read_pinned_ids(&dir).unwrap().unwrap();
    assert!(ids.contains("firefox") && ids.contains("nemo"));
    assert!(pinned_ids_mtime(&dir).is_some());
    remove_pinned_ids(&dir).unwrap();
    remove_pinned_ids(&dir).unwrap(); // sem o arquivo, também
    assert_eq!(read_pinned_ids(&dir).unwrap(), None);
}

#[test]
fn area_for_the_grid() {
    let dir = tmpdir("area");
    write_area(&dir, &[(574, 650, 132, 70), (1854, 1010, 132, 70)]).unwrap();
    // O formato que a grade lê (o read_area do swaydesk-drawer-logic e do sway_dock.py)
    assert_eq!(std::fs::read_to_string(area_path(&dir)).unwrap(), "574 650 132 70\n1854 1010 132 70\n");
    remove_area(&dir).unwrap();
    remove_area(&dir).unwrap(); // sem o arquivo, também
    assert!(!area_path(&dir).exists());
    assert_eq!(drawer_shown_path(&dir), dir.join("sway-drawer.shown"));
}

#[test]
fn toggle_move_and_drop() {
    assert_eq!(toggle_pin(&s(&["a", "b"]), "a"), s(&["b"]));
    assert_eq!(toggle_pin(&s(&["a"]), "b"), s(&["a", "b"]));
    assert_eq!(move_pin(&s(&["a", "b", "c"]), "c", 0), s(&["c", "a", "b"]));
    assert_eq!(move_pin(&s(&["a", "b", "c"]), "a", 99), s(&["b", "c", "a"]));
    assert_eq!(move_pin(&s(&["a", "b", "c"]), "b", -3), s(&["b", "a", "c"]));
    assert_eq!(drop_index(10.0, &[40.0, 40.0, 40.0]), 0);
    assert_eq!(drop_index(25.0, &[40.0, 40.0, 40.0]), 1);
    assert_eq!(drop_index(500.0, &[40.0, 40.0, 40.0]), 2);
    assert_eq!(drop_index(5.0, &[]), 0);
}

#[test]
fn icon_paths_reexported() {
    assert_eq!(icon_paths("/x/a.png"), vec!["/x/a.png".to_string()]);
    assert_eq!(icon_paths("/x/org.example.App").len(), 4);
    let _: HashMap<(), ()> = HashMap::new();
}
