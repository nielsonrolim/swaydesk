//! Os mesmos casos do tests/test_lock_logic.py.
use super::*;
use std::time::Duration;

fn tm(year: i32, month: i32, day: i32, hour: i32, min: i32) -> libc::tm {
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    t.tm_year = year - 1900;
    t.tm_mon = month - 1;
    t.tm_mday = day;
    t.tm_hour = hour;
    t.tm_min = min;
    unsafe { libc::timegm(&mut t) }; // completa o dia da semana
    t
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swaydesk-lock-logic-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Cria {caminho relativo: conteúdo} (None: uma pasta) debaixo de base.
fn tree(base: &Path, files: &[(&str, Option<&str>)]) {
    for (rel, content) in files {
        let path = base.join(rel);
        match content {
            None => std::fs::create_dir_all(&path).unwrap(),
            Some(text) => write(&path, format!("{text}\n").as_bytes()),
        }
    }
}

fn user(home: &Path, gecos: &str) -> User {
    User { name: "ana".into(), gecos: gecos.into(), dir: home.to_path_buf() }
}

#[test]
fn time_and_date() {
    let now = tm(2026, 11, 12, 9, 5);
    assert_eq!(time_text(&now), "09:05");
    assert_eq!(date_format("pt_BR"), "%A, %-d de %B");
    assert_eq!(date_format("es_MX"), "%A, %-d de %B");
    assert_eq!(date_format("es"), "%A, %-d de %B");
    assert_eq!(date_format("en_US"), "%A, %B %-d");
    assert_eq!(date_format(""), "%A, %B %-d");
    // O processo de teste fica no locale C
    assert_eq!(date_text(&now, "en_US"), "Thursday, November 12");
}

#[test]
fn initials_of_the_name() {
    assert_eq!(initials("Ana Maria Souza"), "AS");
    assert_eq!(initials("  ana "), "A");
    assert_eq!(initials("élia"), "É");
    assert_eq!(initials(""), "");
    assert_eq!(initials("(x)"), "");
}

#[test]
fn name_from_accounts_service_then_gecos_then_login() {
    let dir = tmp("name");
    let accounts = dir.join("accounts");
    write(&accounts.join("users/ana"), b"[User]\nRealName=Ana S.\nIcon=/x\n");
    assert_eq!(real_name(&user(&dir, "Ana Souza,,,"), &accounts), "Ana S.");
    write(&accounts.join("users/ana"), b"[User]\nRealName=\n");
    assert_eq!(real_name(&user(&dir, "Ana Souza,,,"), &accounts), "Ana Souza");
    assert_eq!(real_name(&user(&dir, ""), &accounts), "ana");
}

#[test]
fn photo_prefers_face_then_accounts_icon() {
    let dir = tmp("photo");
    let (accounts, home) = (dir.join("accounts"), dir.join("home"));
    std::fs::create_dir_all(accounts.join("icons")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let u = user(&home, "");
    assert_eq!(photo(&u, &accounts), None);
    let icon = accounts.join("icons/ana");
    write(&icon, b"png");
    assert_eq!(photo(&u, &accounts), Some(icon.clone()));
    write(&home.join(".face"), b""); // vazio não conta
    assert_eq!(photo(&u, &accounts), Some(icon));
    write(&home.join(".face"), b"jpg");
    assert_eq!(photo(&u, &accounts), Some(home.join(".face")));
}

#[test]
fn current_user_is_from_getpwuid() {
    // Num contêiner com -u UID sem a linha no /etc/passwd não há usuário: -v /etc/passwd:/etc/passwd:ro
    let me = current_user().expect("no passwd entry for this UID (in a container, mount /etc/passwd)");
    assert!(!me.name.is_empty());
}

#[test]
fn blur_key_follows_the_real_file_and_the_monitor() {
    let dir = tmp("key");
    let image = dir.join("dia.jpg");
    write(&image, &[b'a'; 10]);
    let link = dir.join("current.jpg");
    std::os::unix::fs::symlink(&image, &link).unwrap();
    let key = blur_key(&link, 1920, 1080).unwrap();
    assert_eq!(key.len(), 32);
    assert_eq!(Some(key.clone()), blur_key(&image, 1920, 1080));
    assert_ne!(Some(key.clone()), blur_key(&link, 1280, 720));
    let other = dir.join("outro.jpg");
    write(&other, &[b'a'; 10]);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&other, &link).unwrap();
    assert_ne!(Some(key), blur_key(&link, 1920, 1080));
}

#[test]
fn blur_key_changes_when_the_file_changes() {
    let dir = tmp("change");
    let image = dir.join("dia.jpg");
    write(&image, &[b'a'; 10]);
    let key = blur_key(&image, 800, 600);
    std::thread::sleep(Duration::from_millis(10));
    write(&image, &[b'b'; 11]);
    assert_ne!(key, blur_key(&image, 800, 600));
}

#[test]
fn blur_key_is_the_one_of_python() {
    // sha256("/x/dia.jpg\0" "10\0" "5000000000\0" "800x600") com o arquivo de verdade: aqui, só a
    // montagem do texto, com um caminho e um mtime conhecidos
    let dir = tmp("python");
    let image = dir.join("dia.jpg");
    write(&image, &[b'a'; 10]);
    let real = std::fs::canonicalize(&image).unwrap();
    let meta = std::fs::metadata(&real).unwrap();
    let raw = format!("{}\0{}\0{}\0800x600", real.display(), meta.size(), meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128);
    let digest: String = Sha256::digest(raw.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(blur_key(&image, 800, 600).unwrap(), digest[..32]);
}

#[test]
fn missing_wallpaper_and_cached_blur_never_generates() {
    let dir = tmp("missing");
    let env: HashMap<String, String> = [("XDG_CACHE_HOME".to_string(), dir.join("cache").display().to_string())].into();
    assert_eq!(blur_key(&dir.join("nada.jpg"), 800, 600), None);
    assert_eq!(cached_blur(&dir.join("nada.jpg"), 800, 600, &env), None);
    let image = dir.join("dia.jpg");
    write(&image, b"a");
    assert_eq!(cached_blur(&image, 800, 600, &env), None);
    assert!(!dir.join("cache").exists());
    let path = blur_path(&image, 800, 600, &env).unwrap();
    assert!(path.starts_with(dir.join("cache/swaydesk")));
    write(&path, b"png");
    assert_eq!(cached_blur(&image, 800, 600, &env), Some(path));
}

#[test]
fn cache_dir_without_xdg() {
    let env: HashMap<String, String> = [("HOME".to_string(), "/home/ana".to_string())].into();
    assert_eq!(cache_dir(&env), PathBuf::from("/home/ana/.cache/swaydesk"));
}

#[test]
fn sizes_of_active_outputs_without_repeats() {
    let outputs = serde_json::json!([
        {"active": true, "rect": {"width": 1920, "height": 1080}},
        {"active": false, "rect": {"width": 1280, "height": 720}},
        {"active": true, "rect": {"width": 1920, "height": 1080}},
        {"active": true, "rect": {"width": 1366, "height": 768}},
        {"active": true, "rect": {"width": 0, "height": 0}}
    ]);
    assert_eq!(output_sizes(&outputs), [(1920, 1080), (1366, 768)]);
    assert_eq!(output_sizes(&serde_json::Value::Null), []);
}

#[test]
fn cover() {
    assert_eq!(cover_size(1920, 1080, 1280, 1024), (1820, 1024));
    assert_eq!(cover_size(1000, 1000, 1920, 1080), (1920, 1920));
}

#[test]
fn prune_keeps_the_newest() {
    let dir = tmp("prune");
    for i in 0..5 {
        write(&dir.join(format!("lock-{i}.png")), b"x");
        std::thread::sleep(Duration::from_millis(10));
    }
    write(&dir.join("other.png"), b"x");
    prune(&dir, 2, Some(&dir.join("lock-0.png")));
    let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["lock-0.png", "lock-3.png", "lock-4.png", "other.png"]);
}

#[test]
fn battery_states() {
    let root = tmp("battery").join("ps");
    assert_eq!(battery(&root), None);
    tree(&root, &[("AC/type", Some("Mains")), ("AC/online", Some("0")), ("BAT0/type", Some("Battery")), ("BAT0/capacity", Some("57")), ("BAT0/status", Some("Discharging"))]);
    assert_eq!(battery(&root), Some((57, Power::Discharging)));
    tree(&root, &[("BAT0/status", Some("Charging"))]);
    assert_eq!(battery(&root), Some((57, Power::Charging)));
    tree(&root, &[("BAT0/status", Some("Not charging")), ("AC/online", Some("1"))]);
    assert_eq!(battery(&root), Some((57, Power::Plugged)));
    tree(&root, &[("BAT0/status", Some("Discharging")), ("AC/online", Some("1"))]);
    assert_eq!(battery(&root), Some((57, Power::Plugged)));
}

#[test]
fn peripheral_batteries_do_not_count() {
    let root = tmp("peripheral").join("ps");
    tree(&root, &[("hidpp_battery_0/type", Some("Battery")), ("hidpp_battery_0/scope", Some("Device")), ("hidpp_battery_0/capacity", Some("90"))]);
    assert_eq!(battery(&root), None);
}

#[test]
fn battery_icons() {
    assert_eq!(battery_icon(100, Power::Discharging), BATTERY_ICONS[4]);
    assert_eq!(battery_icon(19, Power::Discharging), BATTERY_ICONS[0]);
    assert_eq!(battery_icon(50, Power::Charging), BATTERY_CHARGING);
    assert_eq!(battery_icon(50, Power::Plugged), BATTERY_PLUGGED);
}

#[test]
fn network_states() {
    let dir = tmp("net");
    let (net, wireless) = (dir.join("net"), dir.join("wireless"));
    tree(&net, &[("lo/operstate", Some("unknown")), ("docker0/operstate", Some("up"))]);
    assert_eq!(network(&net, &wireless), Net::Offline);
    tree(&net, &[("enp3s0/device", None), ("enp3s0/operstate", Some("up"))]);
    assert_eq!(network(&net, &wireless), Net::Ethernet);
    tree(&net, &[("wlp2s0/device", None), ("wlp2s0/wireless", None), ("wlp2s0/operstate", Some("up"))]);
    write(&wireless, b"Inter-| sta-|   Quality\n face | tus | link level noise\n wlp2s0: 0000   49.  -61.  -256 0 0 0 0 0 0\n");
    assert_eq!(network(&net, &wireless), Net::Wifi(70));
    tree(&net, &[("wlp2s0/operstate", Some("down"))]);
    assert_eq!(network(&net, &wireless), Net::Ethernet);
}

#[test]
fn network_icons() {
    assert_eq!(network_icon(Net::Wifi(10)), WIFI_ICONS[0]);
    assert_eq!(network_icon(Net::Wifi(80)), WIFI_ICONS[3]);
    assert_eq!(network_icon(Net::Ethernet), ETHERNET_ICON);
    assert_eq!(network_icon(Net::Offline), OFFLINE_ICON);
}

#[test]
fn keyboard_layouts() {
    let inputs = serde_json::json!([{"type": "pointer"}, {"type": "keyboard", "xkb_active_layout_name": "Portuguese (Brazil)", "xkb_layout_names": ["Portuguese (Brazil)", "English (US)"]}]);
    assert_eq!(keyboard_layout(&inputs), ("Portuguese (Brazil)".to_string(), true));
    let one = serde_json::json!([{"type": "keyboard", "xkb_active_layout_name": "English (US)", "xkb_layout_names": ["English (US)"]}]);
    assert_eq!(keyboard_layout(&one), ("English (US)".to_string(), false));
    assert_eq!(keyboard_layout(&serde_json::json!([])), (String::new(), false));
}

#[test]
fn local_art_never_from_the_network() {
    let dir = tmp("art");
    let cache = dir.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let cover = dir.join("capa.jpg");
    write(&cover, b"jpg");
    let uri = format!("file://{}", cover.display());
    assert_eq!(local_art(&uri, &cache), Some(cover));
    let url = "https://example.com/capa.jpg";
    assert_eq!(local_art(url, &cache), None); // não está no cache: nada de baixar
    let cached = art::art_path(url, &cache).unwrap();
    write(&cached, b"jpg");
    assert_eq!(local_art(url, &cache), Some(cached));
    assert_eq!(local_art("data:image/png;base64,AAAA", &cache), None);
    assert_eq!(local_art("", &cache), None);
    assert_eq!(local_art(&format!("file://{}", dir.join("nada.jpg").display()), &cache), None);
}

#[test]
fn art_bytes_only_accepted_formats_and_never_the_network() {
    let dir = tmp("artbytes");
    let cache = dir.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let png = b"\x89PNG\r\n\x1a\nresto".to_vec();
    let file = dir.join("capa.png");
    write(&file, &png);
    assert_eq!(art_bytes(&format!("file://{}", file.display()), &cache), Some((png.clone(), "png")));
    let ico = dir.join("capa.ico");
    write(&ico, b"\x00\x00\x01\x00");
    assert_eq!(art_bytes(&format!("file://{}", ico.display()), &cache), None);
    // "\x89PNG\r\n\x1a\nresto" em base64
    assert_eq!(art_bytes("data:image/jpeg;base64,iVBORw0KGgpyZXN0bw==", &cache), Some((png, "png")));
    assert_eq!(art_bytes("https://example.com/capa.png", &cache), None);
    assert_eq!(art_bytes("", &cache), None);
}

#[test]
fn the_first_generated_time_locale_wins() {
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // Um que nenhum sistema gera: a data fica no C, e a linha diz qual faltou
    let (used, missing) = set_time_locale(&names(&["xx_XX"]));
    assert_eq!((used.as_str(), missing.clone()), ("", names(&["xx_XX"])));
    assert_eq!(missing_time_locale_line(&missing, &used).unwrap(), "xx_XX.UTF-8 is not generated: the date uses C");
    // O C.UTF-8 existe em toda glibc atual: o segundo da lista entra
    let (used, missing) = set_time_locale(&names(&["xx_XX", "C"]));
    assert_eq!((used.as_str(), missing), ("C", names(&["xx_XX"])));
    assert_eq!(
        missing_time_locale_line(&names(&["xx_XX", "yy_YY"]), "C").unwrap(),
        "xx_XX.UTF-8, yy_YY.UTF-8 are not generated: the date uses C.UTF-8"
    );
    assert_eq!(missing_time_locale_line(&[], "pt_BR"), None);
    // O C escolhido (nenhum candidato): o C, sem linha no log
    let (used, missing) = set_time_locale(&[]);
    assert_eq!(missing_time_locale_line(&missing, &used), None);
    unsafe { libc::setlocale(libc::LC_TIME, c"C".as_ptr()) };
}
