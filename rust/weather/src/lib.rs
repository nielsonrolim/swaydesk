//! O tempo que o ícone da Waybar e o popup compartilham, sem GTK: o porte de
//! `home/.local/bin/waybar_weather.py` (a busca no wttr.in e o cache).
//!
//! Os dois leem o mesmo arquivo para mostrar o mesmo número: o wttr.in responde com medições de horários
//! diferentes conforme a URL, então buscas separadas discordam. O cache é o mesmo do Python
//! (`$XDG_RUNTIME_DIR/waybar-weather.json`): as duas versões convivem.
mod icons;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gio::glib;
use serde_json::Value;
use swaydesk_common::{config, resident, text};

pub use icons::{ICONS, NIGHT};

pub const CACHE_TTL: f64 = 15.0 * 60.0;
/// Depois de 6 h sem resposta, o ícone não mostra o valor antigo
pub const EXPIRE: f64 = 6.0 * 60.0 * 60.0;
pub const PLACEHOLDER: &str = "--°";
/// Igual ao "signal" de custom/weather na config da Waybar
pub const BAR_SIGNAL: u32 = 8;

/// A cidade, a língua e o arquivo do cache.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Vazio: a localização vem do IP
    pub location: String,
    /// Vazio: o wttr.in responde em inglês
    pub lang: String,
    pub cache: PathBuf,
}

impl Settings {
    pub fn load() -> Settings {
        let config = config::load();
        let env: HashMap<String, String> = std::env::vars().collect();
        Settings {
            location: config.weather_location.clone(),
            lang: config::language(&config, &env),
            cache: resident::runtime_dir().join("waybar-weather.json"),
        }
    }
}

/// Agora, no fuso local: os segundos desde 1970, a data (AAAA-MM-DD) e os minutos desde a meia-noite.
#[derive(Clone, Debug)]
pub struct Now {
    pub epoch: f64,
    pub date: String,
    pub minutes: u32,
}

impl Now {
    pub fn local() -> Now {
        let now = glib::DateTime::now_local().expect("hora local");
        Now {
            epoch: glib::real_time() as f64 / 1e6,
            date: now.format("%Y-%m-%d").map(|s| s.to_string()).unwrap_or_default(),
            minutes: (now.hour() * 60 + now.minute()) as u32,
        }
    }
}

/// A descrição na língua pedida; o wttr.in nem sempre tem a tradução.
pub fn description(item: &Value, lang: &str) -> Option<String> {
    let translated = item.get(format!("lang_{lang}")).filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));
    translated.or_else(|| item.get("weatherDesc"))?.get(0)?.get("value")?.as_str().map(String::from)
}

/// Uma data AAAA-MM-DD válida (o `date.fromisoformat`).
pub fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts.as_slice() else { return false };
    let num = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    if !(num(y, 4) && num(m, 2) && num(d, 2)) {
        return false;
    }
    let (y, m, d): (i32, u32, u32) = (y.parse().unwrap_or(0), m.parse().unwrap_or(0), d.parse().unwrap_or(0));
    glib::DateTime::from_utc(y, m as i32, d as i32, 0, 0, 0.0).is_ok() && y >= 1
}

fn int_of(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

/// Erro se faltar algo que o desenho usa: uma resposta ruim não entra no cache.
pub fn check(data: &Value, lang: &str) -> Result<(), String> {
    let missing = |what: &str| format!("missing {what}");
    let cur = data.get("current_condition").and_then(|c| c.get(0)).ok_or_else(|| missing("current_condition"))?;
    for key in ["weatherCode", "temp_C", "FeelsLikeC", "humidity", "windspeedKmph", "winddir16Point"] {
        cur.get(key).ok_or_else(|| missing(key))?;
    }
    description(cur, lang).ok_or_else(|| missing("weatherDesc"))?;
    let days = data.get("weather").and_then(Value::as_array).ok_or_else(|| missing("weather"))?;
    if days.is_empty() {
        return Err("no forecast".into());
    }
    for day in days.iter().take(3) {
        if !day.get("date").and_then(Value::as_str).is_some_and(is_date) {
            return Err(missing("date"));
        }
        for key in ["mintempC", "maxtempC"] {
            day.get(key).ok_or_else(|| missing(key))?;
        }
        let ast = day.get("astronomy").and_then(|a| a.get(0)).ok_or_else(|| missing("astronomy"))?;
        ast.get("sunrise").and(ast.get("sunset")).ok_or_else(|| missing("sunrise"))?;
        let hourly = day.get("hourly").and_then(Value::as_array).ok_or_else(|| missing("hourly"))?;
        if hourly.len() < 5 {
            return Err("incomplete forecast".into());
        }
        for h in hourly {
            int_of(h.get("time")).ok_or_else(|| missing("time"))?;
            int_of(h.get("chanceofrain")).ok_or_else(|| missing("chanceofrain"))?;
            h.get("weatherCode").and(h.get("tempC")).ok_or_else(|| missing("tempC"))?;
            description(h, lang).ok_or_else(|| missing("weatherDesc"))?;
        }
    }
    Ok(())
}

/// O `urllib.parse.quote` (o "/" fica).
pub fn quote(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// O endereço do wttr.in: a cidade fica no caminho, e o idioma é opcional.
pub fn weather_url(settings: &Settings, extra: &str) -> String {
    let query = if settings.lang.is_empty() { extra.to_string() } else { format!("{extra}&lang={}", settings.lang) };
    format!("https://wttr.in/{}?{query}", quote(&settings.location))
}

fn get(url: &str, timeout: Duration) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    agent.get(url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
}

/// Baixa o JSON do wttr.in e o nome do lugar e grava no cache. Bloqueante: fora da linha da interface.
pub fn fetch(settings: &Settings) -> Result<Value, String> {
    let body = get(&weather_url(settings, "format=j1"), Duration::from_secs(15))?;
    let place = get(&format!("https://wttr.in/{}?format=%l", quote(&settings.location)), Duration::from_secs(10)).ok();
    store(settings, &body, place.as_deref())
}

/// Confere a resposta e grava no cache, sem nunca deixar um arquivo pela metade (o ícone e o popup leem
/// a qualquer hora).
pub fn store(settings: &Settings, body: &str, place: Option<&str>) -> Result<Value, String> {
    let mut data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    check(&data, &settings.lang)?;
    if let Some(place) = place {
        data["_place"] = Value::from(place.trim());
    }
    data["_fetched"] = Value::from(glib::real_time() as f64 / 1e6);
    // A cidade pedida: outra, depois, não usa este cache
    data["_location"] = Value::from(settings.location.as_str());
    text::atomic_write(&settings.cache, data.to_string().as_bytes(), 0o644).map_err(|e| e.to_string())?;
    Ok(data)
}

/// O cache válido, e se ainda é fresco (menos de 15 min, e do dia de hoje).
pub fn cached(settings: &Settings, now: &Now) -> (Option<Value>, bool) {
    let Ok(text) = std::fs::read_to_string(&settings.cache) else { return (None, false) };
    let Ok(data) = serde_json::from_str::<Value>(&text) else { return (None, false) };
    if check(&data, &settings.lang).is_err() {
        return (None, false);
    }
    // Outra cidade (o weather_location mudou): o cache é o da de antes, e não vale; sem a chave, o do IP
    if data.get("_location").and_then(Value::as_str).unwrap_or("") != settings.location {
        return (None, false);
    }
    // Virou o dia: os dados de ontem não contam como frescos
    let fresh = now.epoch - fetched(&data) < CACHE_TTL && data["weather"][0]["date"].as_str() == Some(now.date.as_str());
    (Some(data), fresh)
}

pub fn fetched(data: &Value) -> f64 {
    data.get("_fetched").and_then(Value::as_f64).unwrap_or(0.0)
}

/// "05:14 PM" → minutos desde a meia-noite (à mão: o %p depende do locale).
pub fn hm(text: &str) -> Option<u32> {
    let (clock, ampm) = text.split_once(' ')?;
    let (h, m) = clock.split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    Some((h % 12 + if ampm.trim().eq_ignore_ascii_case("PM") { 12 } else { 0 }) * 60 + m)
}

/// O nascer e o pôr do sol do dia, em minutos desde a meia-noite (6:00 e 18:00 sem dado).
pub fn sun_minutes(day: &Value) -> (u32, u32) {
    let ast = &day["astronomy"][0];
    match (ast["sunrise"].as_str().and_then(hm), ast["sunset"].as_str().and_then(hm)) {
        (Some(rise), Some(set)) => (rise, set),
        _ => (6 * 60, 18 * 60),
    }
}

pub fn icon_for(code: &str, mins: u32, rise: u32, sset: u32) -> &'static str {
    let night = !(rise <= mins && mins < sset);
    let lookup = |table: &[(&str, &'static str)]| table.iter().find(|(c, _)| *c == code).map(|(_, i)| *i);
    (if night { lookup(NIGHT) } else { None }).or_else(|| lookup(ICONS)).unwrap_or("\u{1f321}\u{fe0f}")
}

fn code(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// O texto do ícone: a mesma condição e a mesma temperatura do topo do popup.
pub fn bar_text(data: &Value, now: &Now) -> Option<String> {
    let cur = &data["current_condition"][0];
    let days = data["weather"].as_array()?;
    let day = days.iter().find(|d| d["date"].as_str().is_some_and(|d| d >= now.date.as_str())).or(days.first())?;
    let (rise, sset) = sun_minutes(day);
    let icon = icon_for(&code(&cur["weatherCode"]), now.minutes, rise, sset);
    let temp = int_of(cur.get("temp_C"))?;
    // O rise centraliza o emoji no texto da Noto Sans (medido com o grim: o mesmo centro do relógio)
    Some(format!("<span size=\"small\" rise=\"1pt\">{icon}</span> {temp}°C"))
}

/// O que o ícone mostra. A Waybar pede a cada minuto: só o tempo do cache decide buscar de novo. Uma busca
/// que falha mantém o último valor (sem saída a Waybar esconderia o módulo). A virada do dia fica de fora:
/// o wttr.in pode demorar a trocar a data, o que faria uma busca por minuto.
pub fn bar_line(settings: &Settings, now: &Now, fetch: impl FnOnce(&Settings) -> Result<Value, String>) -> String {
    let (mut data, _fresh) = cached(settings, now);
    if data.as_ref().is_none_or(|d| now.epoch - fetched(d) >= CACHE_TTL) {
        if let Ok(new) = fetch(settings) {
            data = Some(new);
        }
    }
    data.filter(|d| now.epoch - fetched(d) < EXPIRE).and_then(|d| bar_text(&d, now)).unwrap_or_else(|| PLACEHOLDER.to_string())
}

/// Depois de uma busca do popup, o ícone não espera o próximo minuto para mostrar o mesmo valor.
pub fn refresh_bar() {
    let _ = std::process::Command::new("pkill")
        .args([&format!("-RTMIN+{BAR_SIGNAL}"), "-u", &swaydesk_common::resident::own_uid(), "-x", "waybar"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests;
