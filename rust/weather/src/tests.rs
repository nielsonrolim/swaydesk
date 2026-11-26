//! Os testes de `tests/test_weather.py`.
use super::*;
use serde_json::json;

fn sample(temp: &str, code: &str, fetched: f64) -> Value {
    let hour = json!({"time": "0", "chanceofrain": "10", "weatherCode": code, "tempC": temp, "weatherDesc": [{"value": "Chuva"}]});
    json!({
        "_fetched": fetched,
        "current_condition": [{"weatherCode": code, "temp_C": temp, "FeelsLikeC": temp, "humidity": "66",
                               "windspeedKmph": "21", "winddir16Point": "ESE", "weatherDesc": [{"value": "Chuva"}]}],
        "weather": [{"date": "2026-11-03", "mintempC": "26", "maxtempC": "28",
                     "astronomy": [{"sunrise": "04:52 AM", "sunset": "05:14 PM"}],
                     "hourly": vec![hour; 8]}],
    })
}

/// Meio-dia de 03/11/2026, com o relógio dos testes
fn noon(epoch: f64) -> Now {
    Now { epoch, date: "2026-11-03".into(), minutes: 12 * 60 }
}

fn settings(name: &str) -> Settings {
    let dir = std::env::temp_dir().join(format!("swaydesk-weather-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Settings { location: String::new(), lang: String::new(), cache: dir.join("weather.json") }
}

const T: f64 = 1_800_000_000.0;

#[test]
fn shows_icon_and_temperature() {
    assert_eq!(bar_text(&sample("28", "176", T), &noon(T)).unwrap(), "<span size=\"small\" rise=\"1pt\">\u{1f326}\u{fe0f}</span> 28°C");
    assert!(bar_text(&sample("-3", "113", T), &noon(T)).unwrap().ends_with(" -3°C"));
}

#[test]
fn night_icon_follows_the_sunset() {
    let mut now = noon(T);
    now.minutes = 17 * 60 + 14; // o pôr do sol
    assert!(bar_text(&sample("20", "113", T), &now).unwrap().contains("\u{1f319}"));
    now.minutes = 17 * 60 + 13;
    assert!(bar_text(&sample("20", "113", T), &now).unwrap().contains("\u{2600}"));
}

#[test]
fn unknown_code_has_an_icon_and_a_bad_temperature_is_refused() {
    assert!(bar_text(&sample("20", "999", T), &noon(T)).unwrap().contains("\u{1f321}"));
    assert!(bar_text(&sample("quente", "113", T), &noon(T)).is_none());
}

#[test]
fn a_cache_for_another_city_fetches_again() {
    // O weather_location mudou: o cache da cidade de antes, ainda fresco, não vale (sem _location: o do IP)
    let mut s = settings("city");
    std::fs::write(&s.cache, sample("27", "176", T).to_string()).unwrap();
    s.location = "Lisbon".into();
    let mut fetched = false;
    let line = bar_line(&s, &noon(T + 60.0), |_| {
        fetched = true;
        let mut new = sample("15", "176", T + 60.0);
        new["_location"] = "Lisbon".into();
        Ok(new)
    });
    assert!(fetched);
    assert!(line.ends_with(" 15°C"), "{line}");
}

#[test]
fn another_city_without_network_does_not_show_the_old_one() {
    let mut s = settings("city-offline");
    let mut old = sample("27", "176", T);
    old["_location"] = "Porto".into();
    std::fs::write(&s.cache, old.to_string()).unwrap();
    s.location = "Lisbon".into();
    assert_eq!(bar_line(&s, &noon(T + 60.0), |_| Err("offline".into())), PLACEHOLDER);
}

#[test]
fn the_same_city_keeps_the_cache() {
    let mut s = settings("city-same");
    let mut old = sample("27", "176", T);
    old["_location"] = "Lisbon".into();
    std::fs::write(&s.cache, old.to_string()).unwrap();
    s.location = "Lisbon".into();
    assert!(bar_line(&s, &noon(T + 60.0), |_| panic!("não devia buscar")).ends_with(" 27°C"));
}

#[test]
fn the_cache_keeps_the_city_it_was_fetched_for() {
    let mut s = settings("city-store");
    s.location = "João Pessoa".into();
    let mut body = sample("27", "176", T);
    body.as_object_mut().unwrap().remove("_fetched");
    let data = store(&s, &body.to_string(), Some("João Pessoa")).unwrap();
    assert_eq!(data["_location"], "João Pessoa");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&s.cache).unwrap()).unwrap();
    assert_eq!(saved["_location"], "João Pessoa");
}

#[test]
fn recent_cache_does_not_use_the_network() {
    let s = settings("recent");
    std::fs::write(&s.cache, sample("28", "176", T).to_string()).unwrap();
    let line = bar_line(&s, &noon(T + 60.0), |_| panic!("não devia buscar"));
    assert!(line.ends_with(" 28°C"), "{line}");
}

#[test]
fn old_cache_fetches_again_and_a_failure_keeps_the_last_value() {
    let s = settings("old");
    std::fs::write(&s.cache, sample("27", "176", T).to_string()).unwrap();
    let later = noon(T + CACHE_TTL);
    assert!(bar_line(&s, &later, |_| Ok(sample("28", "176", T + CACHE_TTL))).ends_with(" 28°C"));
    assert!(bar_line(&s, &later, |_| Err("offline".into())).ends_with(" 27°C"));
    // Expirado e sem rede: o marcador
    assert_eq!(bar_line(&s, &noon(T + EXPIRE), |_| Err("offline".into())), PLACEHOLDER);
}

#[test]
fn no_cache_and_no_network_shows_the_placeholder() {
    assert_eq!(bar_line(&settings("none"), &noon(T), |_| Err("offline".into())), PLACEHOLDER);
}

#[test]
fn new_day_alone_does_not_fetch_again() {
    let s = settings("day");
    let mut data = sample("28", "176", T);
    data["weather"][0]["date"] = json!("2020-01-01");
    std::fs::write(&s.cache, data.to_string()).unwrap();
    bar_line(&s, &noon(T + 60.0), |_| panic!("não devia buscar"));
}

#[test]
fn store_saves_a_good_answer_and_refuses_a_bad_one() {
    let s = settings("store");
    let mut data = sample("28", "176", T);
    data.as_object_mut().unwrap().remove("_fetched");
    let saved = store(&s, &data.to_string(), Some("João Pessoa\n")).unwrap();
    assert_eq!(saved["_place"], "João Pessoa");
    let names: Vec<String> = std::fs::read_dir(s.cache.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["weather.json"]);
    std::fs::write(&s.cache, "antes").unwrap();
    assert!(store(&s, r#"{"erro": 1}"#, None).is_err());
    assert_eq!(std::fs::read_to_string(&s.cache).unwrap(), "antes");
}

#[test]
fn urls_and_descriptions() {
    let mut s = settings("url");
    s.location = "São Paulo".into();
    s.lang = "pt".into();
    assert_eq!(weather_url(&s, "format=j1"), "https://wttr.in/S%C3%A3o%20Paulo?format=j1&lang=pt");
    s.location.clear();
    s.lang.clear();
    assert_eq!(weather_url(&s, "format=j1"), "https://wttr.in/?format=j1");
    let item = json!({"lang_es": [{"value": "Lluvia"}], "lang_pt": [{"value": "Chuva"}], "weatherDesc": [{"value": "Rain"}]});
    assert_eq!(description(&item, "es").as_deref(), Some("Lluvia"));
    assert_eq!(description(&json!({"weatherDesc": [{"value": "Rain"}]}), "fr").as_deref(), Some("Rain"));
}

#[test]
fn times_and_dates() {
    assert_eq!(hm("05:14 PM"), Some(17 * 60 + 14));
    assert_eq!(hm("12:00 AM"), Some(0));
    assert_eq!(hm("12:30 PM"), Some(12 * 60 + 30));
    assert_eq!(hm("xx"), None);
    assert!(is_date("2026-11-03") && !is_date("2026-13-01") && !is_date("26-11-03"));
}

#[test]
fn icons_match_python() {
    let Some(path) = swaydesk_common::reference::repo_file(env!("CARGO_MANIFEST_DIR"), "home/.local/bin/waybar_weather.py") else { return };
    let python = std::fs::read_to_string(path).unwrap();
    for (code, icon) in ICONS.iter().chain(NIGHT) {
        assert!(python.contains(&format!("\"{code}\": \"{icon}\"")), "{code}");
    }
    assert!(python.contains(&format!("BAR_SIGNAL = {BAR_SIGNAL}")));
    assert!(python.contains(&format!("PLACEHOLDER = \"{PLACEHOLDER}\"")));
}
