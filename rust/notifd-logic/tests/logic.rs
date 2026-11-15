//! O porte dos testes de tests/test_notifications.py (menos o corpo, em body.rs).
use std::cell::Cell;
use std::rc::Rc;

use serde_json::json;
use swaydesk_notifd_logic::*;

fn clock() -> (Rc<Cell<f64>>, Clock) {
    let now = Rc::new(Cell::new(1000.0));
    let shared = now.clone();
    (now, Box::new(move || shared.get()))
}

fn center(max_items: usize) -> Center {
    Center::new(clock().1, max_items)
}

fn summary(text: &str) -> Incoming {
    Incoming { summary: text.into(), ..Default::default() }
}

fn hints(pairs: &[(&str, Hint)]) -> Hints {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------- parse

#[test]
fn plain_is_one_line() {
    assert_eq!(plain("a\n  b\x02", MAX_TEXT), "a b");
}

#[test]
fn actions_become_pairs_and_odd_one_is_dropped() {
    assert_eq!(
        parse_actions(&strings(&["a", "A", "b", "B", "c"])),
        [("a".to_string(), "A".to_string()), ("b".to_string(), "B".to_string())]
    );
}

#[test]
fn actions_have_a_limit() {
    let actions: Vec<String> = (0..MAX_ACTIONS * 4).map(|n| n.to_string()).collect();
    assert_eq!(parse_actions(&actions).len(), MAX_ACTIONS);
}

#[test]
fn action_with_empty_key_is_dropped() {
    assert_eq!(parse_actions(&strings(&["", "A", "b", "B"])).len(), 1);
}

#[test]
fn invalid_urgency_is_normal() {
    for value in [Some(Hint::Int(7)), Some(Hint::Str("2".into())), None, Some(Hint::Int(-1))] {
        assert_eq!(parse_urgency(value.as_ref()), NORMAL, "{value:?}");
    }
    assert_eq!(parse_urgency(Some(&Hint::Int(2))), CRITICAL);
}

#[test]
fn timeouts() {
    for (urgency, expire, expected) in
        [(LOW, -1, 5000), (NORMAL, -1, 10000), (CRITICAL, -1, 0), (NORMAL, 0, 0), (NORMAL, 1500, 1500), (CRITICAL, 1500, 1500)]
    {
        assert_eq!(toast_ms(urgency, expire), expected);
    }
}

#[test]
fn local_paths() {
    assert_eq!(local_path("file:///tmp/a%20b.png").as_deref(), Some("/tmp/a b.png"));
    assert_eq!(local_path("file://localhost/tmp/a.png").as_deref(), Some("/tmp/a.png"));
    assert_eq!(local_path("file:///tmp/a.png?x#y").as_deref(), Some("/tmp/a.png"));
    assert_eq!(local_path("file:///tmp/%zz%C3%A9").as_deref(), Some("/tmp/%zzé"));
    assert_eq!(local_path("/tmp/a.png").as_deref(), Some("/tmp/a.png"));
    assert_eq!(local_path("file://outro/a.png"), None);
    assert_eq!(local_path("firefox"), None);
    assert_eq!(local_path("https://x/y.png"), None);
}

// ---------------------------------------------------------------- ícone

struct Tmp(std::path::PathBuf);

impl Tmp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("swaydesk-notifd-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn icon_order() {
    let tmp = Tmp::new("order");
    let image = tmp.0.join("a.png");
    std::fs::write(&image, b"x").unwrap();
    let image = image.to_str().unwrap();
    assert_eq!(icon_spec("/run/1.png", image, "firefox", "ent"), "/run/1.png");
    assert_eq!(icon_spec("", image, "firefox", "ent"), image);
    assert_eq!(icon_spec("", "", "firefox", "ent"), "firefox");
    assert_eq!(icon_spec("", "", "", "ent"), "ent");
    assert_eq!(icon_spec("", "", "", ""), "");
}

#[test]
fn missing_file_falls_through() {
    assert_eq!(icon_spec("", "/nao/existe.png", "firefox", ""), "firefox");
}

#[test]
fn fifo_is_refused() {
    let tmp = Tmp::new("fifo");
    let fifo = tmp.0.join("fifo");
    let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc_mkfifo(path.as_ptr(), 0o600) }, 0);
    assert_eq!(icon_spec("", fifo.to_str().unwrap(), "", ""), "");
}

extern "C" {
    #[link_name = "mkfifo"]
    fn libc_mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
}

#[test]
fn remote_and_relative_are_refused() {
    assert_eq!(icon_spec("", "https://x/y.png", "../../a", ""), "");
}

#[test]
fn theme_name_is_cut() {
    assert_eq!(icon_spec("", "", &"x".repeat(MAX_TEXT * 2), "").chars().count(), MAX_TEXT);
}

#[test]
fn valid_images() {
    assert!(valid_image(2, 2, 6, false, 8, 3, 12));
    assert!(valid_image(2, 2, 8, true, 8, 4, 16));
    assert!(valid_image(2, 2, 8, false, 8, 3, 14)); // a última linha pode vir sem sobra
}

#[test]
fn invalid_images() {
    let big = MAX_IMAGE_BYTES;
    for args in [
        (0, 2, 6, false, 8, 3, 12), // sem largura
        (MAX_IMAGE_SIDE + 1, 1, 99999, false, 8, 3, 99999),
        (2, 2, 6, false, 16, 3, 12), // bits
        (2, 2, 6, true, 8, 3, 12),   // canais não batem com alfa
        (2, 2, 5, false, 8, 3, 12),  // linha menor que a largura
        (2, 2, 6, false, 8, 3, 11),  // buffer curto
        (1, 1, 3, false, 8, 3, big + 1),
        (1, 1, (1 << 31) - 1, true, 8, 4, 4), // só o tamanho declarado é enorme
        (1, 2, big - 3, false, 8, 3, big),
        (1, MAX_IMAGE_SIDE, i64::MAX, false, 8, 3, 3), // não estoura a conta
    ] {
        let (w, h, r, a, b, c, s) = args;
        assert!(!valid_image(w, h, r, a, b, c, s), "{args:?}");
    }
}

// ---------------------------------------------------------------- a lista

#[test]
fn ids_start_at_one_and_grow() {
    let mut c = center(MAX_ITEMS);
    assert_eq!(c.add(summary("a")).0.id, 1);
    assert_eq!(c.add(summary("b")).0.id, 2);
}

#[test]
fn replace_by_id_keeps_id_and_moves_to_newest() {
    let mut c = center(MAX_ITEMS);
    let first = c.add(summary("a")).0;
    c.add(summary("b"));
    let again = c.add(Incoming { replaces_id: first.id, ..summary("c") }).0;
    assert_eq!(again.id, first.id);
    assert_eq!(c.visible().iter().map(|i| i.summary.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    assert!(again.revision > first.revision);
}

#[test]
fn replace_of_unknown_id_gets_a_new_id() {
    let mut c = center(MAX_ITEMS);
    assert_eq!(c.add(Incoming { replaces_id: 99, ..summary("a") }).0.id, 1);
}

#[test]
fn sync_key_replaces_within_the_same_app() {
    let mut c = center(MAX_ITEMS);
    let h = hints(&[("x-canonical-private-synchronous", Hint::Str("battery".into()))]);
    let first = c.add(Incoming { app_name: "Bateria".into(), hints: h.clone(), ..summary("a") }).0;
    let again = c.add(Incoming { app_name: "Bateria".into(), hints: h, ..summary("b") }).0;
    assert_eq!(first.id, again.id);
    assert_eq!(c.visible().len(), 1);
}

#[test]
fn sync_key_does_not_cross_apps() {
    let mut c = center(MAX_ITEMS);
    let h = hints(&[("synchronous", Hint::Str("k".into()))]);
    let first = c.add(Incoming { app_name: "A".into(), hints: h.clone(), ..Default::default() }).0;
    let other = c.add(Incoming { app_name: "B".into(), hints: h, ..Default::default() }).0;
    assert_ne!(first.id, other.id);
}

#[test]
fn transient_stays_out_of_the_list_but_can_be_found() {
    let mut c = center(MAX_ITEMS);
    let item = c.add(Incoming { hints: hints(&[("transient", Hint::Bool(true))]), ..summary("a") }).0;
    assert!(c.visible().is_empty());
    assert_eq!(c.get(item.id), Some(&item));
}

#[test]
fn clear_keeps_transient_on_screen() {
    let mut c = center(MAX_ITEMS);
    let item = c.add(Incoming { hints: hints(&[("transient", Hint::Bool(true))]), ..summary("a") }).0;
    c.add(summary("b"));
    assert_eq!(c.clear().iter().map(|i| i.summary.as_str()).collect::<Vec<_>>(), ["b"]);
    assert!(c.get(item.id).is_some());
}

#[test]
fn only_a_positive_timeout_is_set_by_the_app() {
    let mut c = center(MAX_ITEMS);
    for (expire, expected) in [(-1, false), (0, false), (5000, true)] {
        assert_eq!(c.add(Incoming { expire_timeout: expire, ..Default::default() }).0.app_timeout, expected);
    }
}

#[test]
fn resident_hint() {
    let mut c = center(MAX_ITEMS);
    assert!(c.add(Incoming { hints: hints(&[("resident", Hint::Bool(true))]), ..Default::default() }).0.resident);
    assert!(c.add(Incoming { hints: hints(&[("resident", Hint::Int(1))]), ..Default::default() }).0.resident);
    assert!(!c.add(Incoming { hints: hints(&[("resident", Hint::Bool(false))]), ..Default::default() }).0.resident);
}

#[test]
fn text_fields_are_cleaned() {
    let mut c = center(MAX_ITEMS);
    let item = c.add(Incoming { app_name: "A\npp".into(), summary: "t\x01 í".into(), body: "<x>".into(), ..Default::default() }).0;
    assert_eq!((item.app_name.as_str(), item.summary.as_str(), item.body.as_str()), ("A pp", "t í", "&lt;x&gt;"));
}

fn critical(text: &str) -> Incoming {
    Incoming { hints: hints(&[("urgency", Hint::Int(CRITICAL as i64))]), ..summary(text) }
}

#[test]
fn trim_drops_oldest_non_critical_first() {
    let mut c = center(2);
    c.add(critical("crit"));
    c.add(summary("old"));
    let evicted = c.add(summary("new")).1;
    assert_eq!(evicted.iter().map(|i| i.summary.as_str()).collect::<Vec<_>>(), ["old"]);
    assert_eq!(c.visible().iter().map(|i| i.summary.as_str()).collect::<Vec<_>>(), ["crit", "new"]);
}

#[test]
fn trim_drops_critical_when_nothing_else_is_left() {
    let mut c = center(1);
    c.add(critical("a"));
    let evicted = c.add(critical("b")).1;
    assert_eq!(evicted.iter().map(|i| i.summary.as_str()).collect::<Vec<_>>(), ["a"]);
}

#[test]
fn id_wraps_and_skips_used() {
    let mut c = center(MAX_ITEMS);
    c.next_id = MAX_ID;
    assert_eq!(c.add(Incoming::default()).0.id, MAX_ID);
    assert_eq!(c.add(Incoming::default()).0.id, 1);
    c.next_id = 1;
    assert_eq!(c.add(Incoming::default()).0.id, 2);
}

#[test]
fn remove_app() {
    let mut c = center(MAX_ITEMS);
    c.add(Incoming { app_name: "A".into(), ..Default::default() });
    c.add(Incoming { app_name: "B".into(), ..Default::default() });
    assert_eq!(c.remove_app("A").iter().map(|i| i.app_name.as_str()).collect::<Vec<_>>(), ["A"]);
    assert_eq!(c.visible().iter().map(|i| i.app_name.as_str()).collect::<Vec<_>>(), ["B"]);
}

#[test]
fn default_action_is_not_a_button() {
    let mut c = center(MAX_ITEMS);
    let item = c.add(Incoming { actions: strings(&["default", "Abrir", "r", "Responder"]), ..Default::default() }).0;
    assert!(item.has_default());
    assert_eq!(item.buttons(), [("r".to_string(), "Responder".to_string())]);
    assert_eq!(item.as_dict()["buttons"], json!([["r", "Responder"]]));
}

#[test]
fn snapshot_is_newest_first() {
    let (_now, clk) = clock();
    let mut c = Center::new(clk, MAX_ITEMS);
    c.add(summary("a"));
    c.add(summary("b"));
    let mut dnd = Dnd::new(clock().1);
    let snap = snapshot(&c, &mut dnd);
    let names: Vec<&str> = snap["items"].as_array().unwrap().iter().map(|i| i["summary"].as_str().unwrap()).collect();
    assert_eq!(names, ["b", "a"]);
    assert_eq!(snap["items"][0]["created_at"], json!(1000.0));
    assert_eq!(snap["dnd"], json!({"active": false, "choice": "off", "until": null}));
}

// ---------------------------------------------------------------- não perturbe

#[test]
fn dnd_starts_off() {
    assert!(!Dnd::new(clock().1).active());
}

#[test]
fn dnd_timed_expires_by_the_clock() {
    let (now, clk) = clock();
    let mut dnd = Dnd::new(clk);
    dnd.set("1h").unwrap();
    assert!(dnd.active());
    now.set(now.get() + 3599.0);
    assert!(dnd.active());
    now.set(now.get() + 1.0);
    assert!(!dnd.active());
    assert_eq!(dnd.as_dict(), json!({"active": false, "choice": "off", "until": null}));
}

#[test]
fn dnd_forever_does_not_expire() {
    let (now, clk) = clock();
    let mut dnd = Dnd::new(clk);
    dnd.set("on").unwrap();
    now.set(now.get() + 1e8);
    assert!(dnd.active());
}

#[test]
fn dnd_toggle() {
    let mut dnd = Dnd::new(clock().1);
    dnd.set("toggle").unwrap();
    assert_eq!(dnd.choice, "on");
    dnd.set("2h").unwrap();
    assert_eq!(dnd.as_dict()["until"], json!(1000.0 + 7200.0));
    dnd.set("toggle").unwrap();
    assert!(!dnd.active());
}

#[test]
fn dnd_unknown_choice_changes_nothing() {
    let mut dnd = Dnd::new(clock().1);
    dnd.set("on").unwrap();
    assert!(dnd.set("3h").is_err());
    assert_eq!(dnd.choice, "on");
}

// ---------------------------------------------------------------- toasts

fn normal_and_critical() -> (Notification, Notification) {
    let mut c = center(MAX_ITEMS);
    (c.add(Incoming::default()).0, c.add(critical("")).0)
}

#[test]
fn normal_shows() {
    assert!(should_toast(&normal_and_critical().0, false, false));
}

#[test]
fn dnd_only_lets_critical_through() {
    let (normal, critical) = normal_and_critical();
    assert!(!should_toast(&normal, true, false));
    assert!(should_toast(&critical, true, false));
}

#[test]
fn open_popup_lets_only_critical_through() {
    // A lista já mostra as outras; uma crítica aparece também com ela aberta (decisão do usuário)
    let (normal, critical) = normal_and_critical();
    assert!(!should_toast(&normal, false, true));
    assert!(should_toast(&critical, false, true));
    assert!(should_toast(&critical, true, true));
}

#[test]
fn queue_limits_what_is_visible() {
    let mut queue = ToastQueue::new(2);
    for n in [1, 2, 3] {
        queue.show(n);
    }
    queue.show(1); // substituição não muda o lugar
    assert_eq!(queue.visible(), [1, 2]);
    queue.hide(1);
    assert_eq!(queue.visible(), [2, 3]);
    assert_eq!(queue.clear(), [2, 3]);
    assert!(queue.visible().is_empty());
}

// ---------------------------------------------------------------- a tela

#[test]
fn groups_follow_most_recent() {
    let items = [json!({"app_name": "B", "id": 3}), json!({"app_name": "A", "id": 2}), json!({"app_name": "B", "id": 1})];
    let groups = group_by_app(&items);
    assert_eq!(groups.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["B", "A"]);
    assert_eq!(groups[0].1.iter().map(|i| i["id"].as_i64().unwrap()).collect::<Vec<_>>(), [3, 1]);
}

#[test]
fn ages() {
    for (delta, expected) in [
        (0.0, Age::Now),
        (59.9, Age::Now),
        (60.0, Age::Minutes(1)),
        (3600.0, Age::Hours(1)),
        (86400.0, Age::Days(1)),
        (172800.0, Age::Days(2)),
        (-5.0, Age::Now),
    ] {
        assert_eq!(age(1000.0, 1000.0 + delta), expected, "{delta}");
    }
}

#[test]
fn icon_states() {
    for (items, active, expected) in [
        (json!([]), false, "none"),
        (json!([1]), false, "notification"),
        (json!([]), true, "dnd-none"),
        (json!([1]), true, "dnd-notification"),
    ] {
        assert_eq!(icon_state(&json!({"items": items, "dnd": {"active": active}})), expected);
    }
}
