//! Popup do tempo para o clique no tempo da Waybar: o porte de `home/.local/bin/waybar-weather`, que passa a
//! vez a este binário (sway_rust). Centralizado logo abaixo da barra, com o tempo agora, as próximas horas
//! e os próximos 3 dias, pelo wttr.in. Os dados vêm do mesmo cache do ícone (swaydesk_weather), para abrir
//! na hora e mostrar o mesmo número. Clicar de novo no tempo, Esc ou clicar fora fecha.
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Instant;

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use serde_json::Value;
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_ui::cmd::spawn_detached;
use swaydesk_ui::{label, run_popup, PopupWindow};
use swaydesk_weather::{self as weather, cached, description, fetched, icon_for, quote, sun_minutes, Now, Settings};

const PROGRAM: &str = "waybar-weather";

const CSS: &str = r#"
.place  { color: #8a8a8f; font-size: 12px; }
.icon   { font-size: 40px; }
.temp   { font-size: 34px; font-weight: bold; color: #ffffff; }
.desc   { font-size: 15px; }
.detail { color: #8a8a8f; font-size: 12px; }
.box    { background: #38383d; border-radius: 10px; padding: 8px 10px; }
.hour   { font-size: 12px; }
.h, .d  { color: #8a8a8f; }
.small-icon { font-size: 18px; }
.rain   { color: @accent; font-size: 11px; }
.day    { font-size: 13px; }
.today  { color: #ffffff; font-weight: bold; }
.foot   { color: #8a8a8f; font-size: 11px; }
.foot link { color: @accent; }
.foot link:hover { color: @accent_hover; }
button.refresh { background: transparent; border: none; box-shadow: none; color: #8a8a8f; padding: 0 4px; min-height: 0; }
button.refresh:hover { color: #ffffff; }
"#;

/// O dia da semana abreviado, de segunda (1) a domingo (7), na língua da interface.
fn weekday(n: i32) -> String {
    let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    ptr("weekday", names[(n.clamp(1, 7) - 1) as usize])
}

/// A rosa dos ventos na língua da interface: em português, ESE vira LSE.
fn wind(direction: &str) -> String {
    direction
        .chars()
        .map(|c| match c {
            'N' | 'E' | 'S' | 'W' => ptr("compass point", &c.to_string()),
            other => other.to_string(),
        })
        .collect()
}

/// O `str.capitalize` do Python: a primeira letra maiúscula, o resto minúsculo.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn int(v: &Value) -> Option<i64> {
    s(v).trim().parse().ok()
}

/// (ano, mês, dia) de "AAAA-MM-DD".
fn ymd(date: &str) -> Option<(i32, i32, i32)> {
    let mut parts = date.split('-').map(|p| p.parse::<i32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

struct Popup {
    popup: Rc<PopupWindow>,
    card: gtk4::Box,
    settings: Settings,
    loading: Cell<bool>,
    data: RefCell<Option<Value>>,
    has_data: Cell<bool>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-weather", CSS, false, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(420, -1);
        popup.window.set_child(Some(&card));
        let this = Rc::new(Popup {
            popup,
            card,
            settings: Settings::load(),
            loading: Cell::new(false),
            data: RefCell::new(None),
            has_data: Cell::new(false),
        });
        let (data, fresh) = cached(&this.settings, &Now::local());
        this.has_data.set(data.is_some());
        match data {
            Some(data) => this.render(&data, !fresh, false),
            None => this.card.append(&label(&tr("Loading…"), &["detail"], 0.5)),
        }
        if !fresh {
            this.reload(false);
        }
        this.popup.show();
        this
    }

    /// Busca em outra linha; um clique em ⟳ com outra busca em andamento não faz nada.
    fn reload(self: &Rc<Self>, clicked: bool) {
        if self.loading.replace(true) {
            return;
        }
        if clicked {
            let data = self.data.borrow().clone();
            if let Some(data) = data {
                self.render(&data, true, false); // mostra que está atualizando
            }
        }
        let (me, settings) = (Rc::downgrade(self), self.settings.clone());
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || weather::fetch(&settings)).await.unwrap_or_else(|_| Err("panic".into()));
            let Some(p) = me.upgrade() else { return };
            p.loading.set(false);
            match result {
                Ok(data) => {
                    weather::refresh_bar();
                    p.render(&data, false, false);
                }
                Err(_) => {
                    let data = p.data.borrow().clone();
                    match (p.has_data.get(), data) {
                        // Sem isso, "atualizando…" ficaria para sempre
                        (true, Some(data)) => p.render(&data, false, true),
                        _ => p.message(&tr("No answer from wttr.in")),
                    }
                }
            }
        });
    }

    fn clear(&self) {
        while let Some(child) = self.card.first_child() {
            self.card.remove(&child);
        }
    }

    fn message(&self, text: &str) {
        self.clear();
        self.card.append(&label(text, &["detail"], 0.5));
    }

    fn render(self: &Rc<Self>, data: &Value, stale: bool, failed: bool) {
        if self.build(data, stale, failed).is_none() {
            self.has_data.set(false);
            self.data.replace(None);
            self.message(&tr("Invalid or outdated data from wttr.in"));
        }
    }

    fn build(self: &Rc<Self>, data: &Value, stale: bool, failed: bool) -> Option<()> {
        let now = Now::local();
        let lang = &self.settings.lang;
        // Só os dias a partir de hoje: um cache de ontem não pode dizer que é "Hoje"
        let days_ahead: Vec<&Value> = data["weather"].as_array()?.iter().filter(|d| d["date"].as_str().is_some_and(|x| x >= now.date.as_str())).collect();
        let today = *days_ahead.first()?;
        let has_today = today["date"].as_str() == Some(now.date.as_str());
        let cur = &data["current_condition"][0];
        let (rise, sset) = sun_minutes(today);
        let icon = |code: &Value, mins: u32| icon_for(&s(code), mins, rise, sset);
        // Monta tudo antes de mexer na janela: um dado ruim mostra a mensagem, sem meio popup
        let mut widgets: Vec<gtk4::Widget> = Vec::new();

        // Lugar
        let place = data["_place"].as_str().filter(|p| !p.is_empty()).map(String::from).unwrap_or_else(|| {
            let area = &data["nearest_area"][0];
            match (area["areaName"][0]["value"].as_str(), area["region"][0]["value"].as_str()) {
                (Some(name), Some(region)) => format!("{name}, {region}"),
                _ => String::new(),
            }
        });
        widgets.push(label(&place, &["place"], 0.0).upcast());

        // Agora
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        top.append(&label(icon(&cur["weatherCode"], now.minutes), &["icon"], 0.5));
        top.append(&label(&format!("{}°", s(&cur["temp_C"])), &["temp"], 0.5));
        let info = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        info.set_valign(gtk4::Align::Center);
        let d = label(&description(cur, lang)?, &["desc"], 0.0);
        d.set_wrap(true);
        d.set_max_width_chars(28);
        info.append(&d);
        info.append(&label(
            &tr("Feels like %(feels)s° · high %(high)s° · low %(low)s°")
                .replace("%(feels)s", &s(&cur["FeelsLikeC"]))
                .replace("%(high)s", &s(&today["maxtempC"]))
                .replace("%(low)s", &s(&today["mintempC"])),
            &["detail"],
            0.0,
        ));
        info.append(&label(
            &tr("Humidity %(humidity)s%% · wind %(speed)s km/h %(direction)s")
                .replace("%(humidity)s", &s(&cur["humidity"]))
                .replace("%%", "%")
                .replace("%(speed)s", &s(&cur["windspeedKmph"]))
                .replace("%(direction)s", &wind(&s(&cur["winddir16Point"]))),
            &["detail"],
            0.0,
        ));
        top.append(&info);
        widgets.push(top.upcast());

        // Próximas horas: blocos de 3 h a partir do atual, hoje e amanhã
        let mut slots = Vec::new();
        for (di, day) in days_ahead.iter().take(2).enumerate() {
            for h in day["hourly"].as_array()? {
                let mins = (int(&h["time"])? / 100 * 60) as u32;
                if di == 0 && has_today && mins + 180 <= now.minutes {
                    continue;
                }
                slots.push((mins, h));
            }
        }
        let hours = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        hours.set_homogeneous(true);
        hours.add_css_class("box");
        for (mins, h) in slots.iter().take(8) {
            let col = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            col.add_css_class("hour");
            col.append(&label(&format!("{:02}h", mins / 60), &["h"], 0.5));
            col.append(&label(icon(&h["weatherCode"], *mins), &["small-icon"], 0.5));
            col.append(&label(&format!("{}°", s(&h["tempC"])), &[], 0.5));
            let rain = int(&h["chanceofrain"])?;
            col.append(&label(&if rain >= 10 { format!("{rain}%") } else { " ".into() }, &["rain"], 0.5));
            hours.append(&col);
        }
        widgets.push(hours.upcast());

        // Próximos dias: a condição do meio-dia, mínima–máxima e a maior chance de chuva
        let days = gtk4::Grid::new();
        days.set_column_spacing(14);
        days.set_row_spacing(6);
        days.add_css_class("box");
        for (i, day) in days_ahead.iter().take(3).enumerate() {
            let date = day["date"].as_str()?;
            let is_today = date == now.date;
            let name = if is_today {
                tr("Today")
            } else {
                let (y, m, d) = ymd(date)?;
                let dt = glib::DateTime::from_local(y, m, d, 12, 0, 0.0).ok()?;
                format!("{} {d:02}", capitalize(&weekday(dt.day_of_week())))
            };
            let hourly = day["hourly"].as_array()?;
            let noon = hourly.get(4)?;
            let rain = hourly.iter().map(|h| int(&h["chanceofrain"])).collect::<Option<Vec<_>>>()?.into_iter().max().unwrap_or(0);
            let desc = label(&description(noon, lang)?, &["day"], 0.0);
            desc.set_hexpand(true);
            desc.set_ellipsize(pango::EllipsizeMode::End);
            let cells: [gtk4::Widget; 5] = [
                label(&name, &["day", if is_today { "today" } else { "d" }], 0.0).upcast(),
                label(icon(&noon["weatherCode"], 12 * 60), &["small-icon"], 0.5).upcast(),
                desc.upcast(),
                label(&format!("{}° – {}°", s(&day["mintempC"]), s(&day["maxtempC"])), &["day"], 1.0).upcast(),
                label(&if rain >= 10 { format!("{rain}%") } else { String::new() }, &["rain"], 1.0).upcast(),
            ];
            for (c, w) in cells.iter().enumerate() {
                days.attach(w, c as i32, i as i32, 1, 1);
            }
        }
        widgets.push(days.upcast());

        // Rodapé: sol, fonte (o link para o wttr.in) e a hora dos dados
        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        foot.append(&label(
            &tr("Sunrise %(rise)s · sunset %(set)s")
                .replace("%(rise)s", &format!("{:02}:{:02}", rise / 60, rise % 60))
                .replace("%(set)s", &format!("{:02}:{:02}", sset / 60, sset % 60)),
            &["foot"],
            0.0,
        ));
        let stamp = if data.get("_fetched").is_some() { fetched(data) } else { now.epoch };
        let upd = glib::DateTime::from_unix_local(stamp as i64).ok().and_then(|t| t.format("%H:%M").ok()).map(|t| t.to_string()).unwrap_or_default();
        let tip = glib::markup_escape_text(&tr("Open in the browser"));
        let link = format!("<a href=\"https://wttr.in/{}\" title=\"{tip}\">wttr.in</a>", quote(&self.settings.location));
        let src = glib::markup_escape_text(&tr("Source: %s")).replace("%s", &link);
        let state = if failed {
            tr("offline (data from %s)")
        } else if stale {
            tr("updating… (data from %s)")
        } else {
            tr("updated at %s")
        }
        .replace("%s", &upd);
        let when = label("", &["foot"], 1.0);
        when.set_markup(&format!("{src} · {}", glib::markup_escape_text(&state)));
        let me = Rc::downgrade(self);
        when.connect_activate_link(move |_, uri| {
            let _ = spawn_detached("xdg-open", &[uri]);
            if let Some(p) = me.upgrade() {
                p.popup.close("link");
            }
            glib::Propagation::Stop
        });
        when.set_hexpand(true);
        foot.append(&when);
        let btn = gtk4::Button::with_label("⟳");
        btn.add_css_class("refresh");
        btn.set_tooltip_text(Some(&tr("Refresh")));
        let me = Rc::downgrade(self);
        btn.connect_clicked(move |_| {
            if let Some(p) = me.upgrade() {
                p.reload(true);
            }
        });
        foot.append(&btn);
        widgets.push(foot.upcast());

        self.has_data.set(true);
        self.data.replace(Some(data.clone()));
        self.popup.rendered_at.set(Instant::now());
        self.clear();
        for w in &widgets {
            self.card.append(w);
        }
        Some(())
    }
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-weather-popup\n",
    "    Opens the weather popup: now, the next hours and the next 3 days (opening it again closes it).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-weather em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), || {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_helpers() {
        assert_eq!(capitalize("SEG"), "Seg");
        assert_eq!(capitalize("mon"), "Mon");
        assert_eq!(ymd("2026-11-03"), Some((2026, 11, 3)));
        assert_eq!(int(&Value::from(" 12")), Some(12));
    }
}
