//! Popup da bateria para o clique na bateria da Waybar: o porte de `home/.local/bin/waybar-battery-popup`,
//! que passa a vez a este binário (sway_rust).
//!
//! Carga e estado, tempo restante, consumo, saúde da bateria, gráfico das últimas 12 horas, perfis de
//! energia (o ativo na cor de destaque) e a bateria de outros aparelhos (fone, mouse), quando houver.
//! Atualiza a cada 5 s (o gráfico a cada minuto), sempre fora da linha da interface: o upower travado
//! não congela o popup. Clicar de novo na bateria, Esc, clicar em outra janela ou trocar de workspace
//! fecha. Dados: o `upower -i` e o GetHistory pelo `busctl`; perfis: o power-profiles-daemon, pelo `busctl`.
mod upower;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Instant;

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use swaydesk_common::i18n::{decimal, ntr, tr};
use swaydesk_common::{config, power, resident};
use swaydesk_ui::cmd::spawn_detached;
use swaydesk_ui::{label, run_popup, PopupWindow};
use upower::{duration, num, parse_upower, py_round, sh, HOURS};

const PROGRAM: &str = "waybar-battery-popup";
const ORANGE: &str = "#f27835";
const RED: &str = "#fc4138";
/// Material da Nerd Font: battery-10..90 e battery
const LEVEL_ICONS: [&str; 10] = [
    "\u{f007a}", "\u{f007b}", "\u{f007c}", "\u{f007d}", "\u{f007e}", "\u{f007f}", "\u{f0080}", "\u{f0081}", "\u{f0082}", "\u{f0079}",
];
/// battery-charging-10..100
const CHARGING_ICONS: [&str; 10] = [
    "\u{f089c}", "\u{f0086}", "\u{f0087}", "\u{f0088}", "\u{f089d}", "\u{f0089}", "\u{f089e}", "\u{f008a}", "\u{f008b}", "\u{f0085}",
];
const PLUG_ICON: &str = "\u{f06a5}";

const CSS: &str = r#"
.big-icon { font-family: "Symbols Nerd Font"; font-size: 40px; }
.pct    { font-size: 34px; font-weight: bold; color: #ffffff; }
.state  { font-size: 15px; }
.detail { color: #8a8a8f; font-size: 12px; }
.box    { background: #38383d; border-radius: 10px; padding: 8px 10px; }
.k      { color: #8a8a8f; font-size: 12px; }
.v      { font-size: 12px; }
.axis   { color: #8a8a8f; font-size: 10px; }
levelbar trough { min-height: 6px; border-radius: 3px; background: #26262a; border: none; padding: 0; }
levelbar block.filled { border-radius: 3px; background: #dadada; border: none; }
levelbar.power block.filled { background: @accent; }
levelbar.warning block.filled { background: #f27835; }
levelbar.critical block.filled { background: #fc4138; }
levelbar block.empty { background: transparent; border: none; }
button.prof {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  padding: 6px 4px; border-radius: 8px; min-height: 0; font-size: 13px;
}
button.prof:hover { background: #4c4c50; }
button.prof.active { background: @accent; color: @on_accent; }
button.prof.active:hover { background: @accent_hover; }
"#;

/// Uma leitura, feita em outra linha.
#[derive(Default)]
struct Reading {
    /// Só na primeira: o caminho da bateria no upower e os outros aparelhos (modelo, carga)
    first: Option<(Option<String>, Vec<(String, String)>)>,
    d: HashMap<String, String>,
    ac: bool,
    profile: String,
    /// Os que o daemon oferece
    profiles: Vec<String>,
    points: Option<Vec<(f64, f64)>>,
}

fn read(first: bool, known: Option<String>, with_history: bool) -> Reading {
    let mut reading = Reading::default();
    let mut bat = known;
    if first {
        let listing = sh(&["upower", "-e"]);
        let paths: Vec<&str> = listing.split_whitespace().collect();
        bat = power::battery_path(&listing);
        let mut others = Vec::new();
        for path in &paths {
            if Some(*path) == bat.as_deref() || path.contains("line_power") || path.contains("DisplayDevice") {
                continue;
            }
            let d = parse_upower(&sh(&["upower", "-i", path]));
            if let (Some(pct), Some(model)) = (d.get("percentage"), d.get("model").filter(|m| !m.is_empty())) {
                others.push((model.clone(), pct.clone()));
            }
        }
        reading.first = Some((bat.clone(), others));
    }
    if let Some(bat) = &bat {
        reading.d = parse_upower(&sh(&["upower", "-i", bat]));
        if with_history {
            reading.points = Some(upower::history(bat));
        }
    }
    reading.ac = upower::on_ac(Path::new("/sys/class/power_supply"));
    reading.profile = power::get_profile();
    reading.profiles = power::get_profiles();
    reading
}

/// A saúde: "87% of the original capacity · 120 cycles", ou "—".
fn health_text(d: &HashMap<String, String>) -> String {
    let field = |k: &str| num(d.get(k).map(String::as_str).unwrap_or(""));
    let mut health = match (field("energy-full"), field("energy-full-design")) {
        (Some(ef), Some(efd)) if ef != 0.0 && efd != 0.0 => {
            tr("%d%% of the original capacity").replace("%d", &(py_round(100.0 * ef / efd) as i64).to_string()).replace("%%", "%")
        }
        _ => "—".into(),
    };
    let cycles = d.get("charge-cycles").map(String::as_str).unwrap_or("N/A");
    if !matches!(cycles, "N/A" | "0" | "") {
        let n = cycles.parse::<u64>().ok().filter(|_| cycles.bytes().all(|b| b.is_ascii_digit())).unwrap_or(2);
        health += " · ";
        health += &ntr("%s cycle", "%s cycles", n).replace("%s", cycles);
    }
    health
}

struct Popup {
    popup: Rc<PopupWindow>,
    accent: String,
    icon: gtk4::Label,
    pct: gtk4::Label,
    state: gtk4::Label,
    time: gtk4::Label,
    level: gtk4::LevelBar,
    vals: HashMap<&'static str, gtk4::Label>,
    chart: gtk4::DrawingArea,
    /// (pontos, carga de agora), lidos pelo desenho
    chart_data: Rc<RefCell<(Vec<(f64, f64)>, Option<f64>)>>,
    prof_buttons: Vec<(&'static str, gtk4::Button)>,
    prof_section: [gtk4::Widget; 2],
    others: gtk4::Box,
    /// A porcentagem, o nível, os detalhes e o gráfico: escondidos sem bateria (um desktop, uma VM)
    battery_parts: [gtk4::Widget; 4],
    bat: RefCell<Option<String>>,
    loaded: Cell<bool>,
    busy: Cell<bool>,
    again: Cell<bool>,
    tick: Cell<u32>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-battery-popup", CSS, true, main_loop);
        let accent = config::palette(&config::load()).accent;
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        card.add_css_class("card");
        card.set_size_request(360, -1);
        popup.window.set_child(Some(&card));

        // Topo: ícone, porcentagem, estado e tempo. Larguras fixas: o ícone e o número variam
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let icon = label("", &["big-icon"], 0.5);
        let pct = label("", &["pct"], 0.5);
        icon.set_size_request(46, -1);
        pct.set_size_request(92, -1);
        let info = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        info.set_valign(gtk4::Align::Center);
        let state = label("", &["state"], 0.0);
        let time = label("", &["detail"], 0.0);
        for (l, chars) in [(&state, 16), (&time, 22)] {
            l.set_width_chars(chars);
            l.set_max_width_chars(chars);
            l.set_ellipsize(pango::EllipsizeMode::End);
            info.append(l);
        }
        top.append(&icon);
        top.append(&pct);
        top.append(&info);
        card.append(&top);

        let level = gtk4::LevelBar::for_interval(0.0, 100.0);
        level.set_mode(gtk4::LevelBarMode::Continuous);
        for name in ["low", "high", "full"] {
            level.remove_offset_value(Some(name)); // sem as cores padrão do tema
        }
        card.append(&level);

        // Detalhes
        let grid = gtk4::Grid::new();
        grid.set_column_spacing(12);
        grid.set_row_spacing(4);
        grid.add_css_class("box");
        let mut vals = HashMap::new();
        for (row, (key, title)) in
            [("rate", tr("Power draw")), ("energy", tr("Energy")), ("health", tr("Health")), ("model", tr("Battery"))].into_iter().enumerate()
        {
            grid.attach(&label(&title, &["k"], 0.0), 0, row as i32, 1, 1);
            let v = label("", &["v"], 1.0);
            v.set_hexpand(true);
            v.set_ellipsize(pango::EllipsizeMode::End);
            v.set_width_chars(30);
            v.set_max_width_chars(30);
            grid.attach(&v, 1, row as i32, 1, 1);
            vals.insert(key, v);
        }
        card.append(&grid);

        // Gráfico
        let chart_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        chart_box.add_css_class("box");
        let chart = gtk4::DrawingArea::new();
        chart.set_content_height(64);
        let chart_data: Rc<RefCell<(Vec<(f64, f64)>, Option<f64>)>> = Rc::default();
        let data = chart_data.clone();
        chart.set_draw_func(move |_, cr, w, h| {
            let (points, now_pct) = &*data.borrow();
            draw_chart(cr, points, *now_pct, f64::from(w), f64::from(h));
        });
        chart_box.append(&chart);
        let axis = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        axis.append(&label(&tr("%d h ago").replace("%d", &HOURS.to_string()), &["axis"], 0.0));
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        axis.append(&spacer);
        axis.append(&label(&tr("now"), &["axis"], 1.0));
        chart_box.append(&axis);
        card.append(&chart_box);

        // Perfis de energia (botões fixos; só a classe "active" muda)
        let prof_label = label(&tr("Power profile"), &["section"], 0.0);
        card.append(&prof_label);
        let prof = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        prof.set_homogeneous(true);
        let mut prof_buttons = Vec::new();
        for (key, name) in [("power-saver", tr("Power saver")), ("balanced", tr("Balanced")), ("performance", tr("Performance"))] {
            let button = gtk4::Button::with_label(&name);
            button.add_css_class("prof");
            prof.append(&button);
            prof_buttons.push((key, button));
        }
        card.append(&prof);

        // Outros aparelhos com bateria (fone, mouse...), preenchidos na primeira leitura
        let others = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        card.append(&others);

        let this = Rc::new(Popup {
            popup,
            accent,
            icon,
            pct: pct.clone(),
            state,
            time,
            level: level.clone(),
            vals,
            chart,
            chart_data,
            prof_buttons,
            prof_section: [prof_label.upcast(), prof.clone().upcast()],
            battery_parts: [pct.clone().upcast(), level.clone().upcast(), grid.upcast(), chart_box.upcast()],
            others,
            bat: RefCell::new(None),
            loaded: Cell::new(false),
            busy: Cell::new(false),
            again: Cell::new(false),
            tick: Cell::new(0),
        });
        for (key, button) in &this.prof_buttons {
            let (me, key) = (Rc::downgrade(&this), *key);
            button.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    p.on_profile(key);
                }
            });
        }

        if glib::find_program_in_path("gnome-power-statistics").is_some() {
            let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            foot.append(&spacer);
            let more = gtk4::Button::with_label(&tr("More information"));
            more.add_css_class("link");
            let me = Rc::downgrade(&this);
            more.connect_clicked(move |_| {
                let _ = spawn_detached("gnome-power-statistics", &[] as &[&str]);
                if let Some(p) = me.upgrade() {
                    p.popup.close("estatísticas");
                }
            });
            foot.append(&more);
            card.append(&foot);
        }

        this.refresh(); // a janela aparece quando a primeira leitura chegar
        let me = Rc::downgrade(&this);
        glib::timeout_add_seconds_local(5, move || match me.upgrade() {
            Some(p) => {
                p.refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        this
    }

    /// Lê em outra linha; se a anterior não terminou, repete quando ela acabar.
    fn refresh(self: &Rc<Self>) {
        if self.busy.get() {
            self.again.set(true);
            return;
        }
        self.busy.set(true);
        let with_history = self.tick.get() % 12 == 0; // o gráfico muda devagar: uma vez por minuto
        self.tick.set(self.tick.get() + 1);
        let (first, known) = (!self.loaded.get(), self.bat.borrow().clone());
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let reading = gio::spawn_blocking(move || read(first, known, with_history)).await.unwrap_or_default();
            if let Some(p) = me.upgrade() {
                p.apply(reading);
            }
        });
    }

    /// Só mexe no texto que mudou, e marca o redesenho para o focus-out logo depois.
    fn put(&self, widget: &gtk4::Label, text: &str) {
        if widget.text() != text {
            widget.set_text(text);
            self.popup.rendered_at.set(Instant::now());
        }
    }

    fn fill_others(&self, others: &[(String, String)]) {
        if others.is_empty() {
            self.others.set_visible(false);
            return;
        }
        self.others.append(&label(&tr("Other devices"), &["section"], 0.0));
        let grid = gtk4::Grid::new();
        grid.set_column_spacing(12);
        grid.set_row_spacing(4);
        grid.add_css_class("box");
        for (row, (name, pct)) in others.iter().enumerate() {
            let n = label(name, &["v"], 0.0);
            n.set_hexpand(true);
            n.set_ellipsize(pango::EllipsizeMode::End);
            grid.attach(&n, 0, row as i32, 1, 1);
            grid.attach(&label(pct, &["v"], 1.0), 1, row as i32, 1, 1);
        }
        self.others.append(&grid);
    }

    fn apply(self: &Rc<Self>, reading: Reading) {
        self.busy.set(false);
        if let Some((bat, others)) = &reading.first {
            self.bat.replace(bat.clone());
            if !self.loaded.get() {
                self.fill_others(others);
            }
        }
        let has_battery = self.bat.borrow().is_some();
        for widget in &self.battery_parts {
            widget.set_visible(has_battery);
        }
        let mut now_pct = None;
        if !has_battery {
            // Um desktop ou uma VM: sem a carga de 0% "descarregando"
            self.icon.set_markup(&format!("<span foreground=\"{}\">{PLUG_ICON}</span>", glib::markup_escape_text(&self.accent)));
            self.put(&self.pct, "");
            self.put(&self.state, &tr("No battery"));
            self.put(&self.time, &tr("Plugged in"));
        } else {
            let d = &reading.d;
            let get = |k: &str| d.get(k).map(String::as_str).unwrap_or("");
            let pct = num(get("percentage")).unwrap_or(0.0);
            let state = get("state");
            let level = ((pct / 10.0).floor() as usize).min(9);
            // Três estados: carregando (bateria com raio, destaque), na tomada sem carregar (tomada,
            // destaque) e na bateria (nível; laranja ≤ 20%, vermelho ≤ 10%)
            let (mode, icon, color, title, time) = if state == "charging" {
                let full = duration(get("time to full"));
                let t = if full.is_empty() { tr("Plugged in") } else { tr("Full in %s").replace("%s", &full) };
                ("charging", CHARGING_ICONS[level], self.accent.as_str(), tr("Charging"), t)
            } else if reading.ac {
                let title = if state == "fully-charged" { tr("Charged") } else { tr("Not charging") };
                ("plugged", PLUG_ICON, self.accent.as_str(), title, tr("Plugged in"))
            } else {
                let color = if pct <= 10.0 { RED } else if pct <= 20.0 { ORANGE } else { "#dadada" };
                let left = duration(get("time to empty"));
                let t = if left.is_empty() { tr("On battery") } else { tr("%s left").replace("%s", &left) };
                ("battery", LEVEL_ICONS[level], color, tr("Discharging"), t)
            };
            self.icon.set_markup(&format!("<span foreground=\"{}\">{icon}</span>", glib::markup_escape_text(color)));
            self.put(&self.pct, &format!("{}%", py_round(pct) as i64));
            self.put(&self.state, &title);
            self.put(&self.time, &time);

            self.level.set_value(pct);
            for class in ["warning", "critical", "power"] {
                self.level.remove_css_class(class);
            }
            if mode != "battery" {
                self.level.add_css_class("power"); // barra no destaque na tomada; cinza-claro na bateria
            } else if pct <= 10.0 {
                self.level.add_css_class("critical");
            } else if pct <= 20.0 {
                self.level.add_css_class("warning");
            }

            let rate = num(get("energy-rate"));
            let rate_text = match rate {
                Some(r) if r > 0.05 && matches!(state, "charging" | "discharging") => format!("{} W", decimal(r, 1)),
                _ => "—".into(),
            };
            self.put(&self.vals["rate"], &rate_text);
            let energy = match (num(get("energy")), num(get("energy-full"))) {
                (Some(e), Some(ef)) if ef != 0.0 => {
                    tr("%(now)s of %(full)s Wh").replace("%(now)s", &decimal(e, 1)).replace("%(full)s", &decimal(ef, 1))
                }
                _ => "—".into(),
            };
            self.put(&self.vals["energy"], &energy);
            self.put(&self.vals["health"], &health_text(d));
            let model: Vec<&str> = ["vendor", "model", "technology"].iter().map(|k| get(k)).filter(|x| !x.is_empty()).collect();
            self.put(&self.vals["model"], &if model.is_empty() { "—".to_string() } else { model.join(" ") });

            now_pct = Some(pct);
        }

        // Só os perfis que o daemon oferece (o ppd sem suporte da plataforma não tem o performance)
        for (key, button) in &self.prof_buttons {
            button.set_visible(reading.profiles.iter().any(|p| p == key));
        }
        for widget in &self.prof_section {
            widget.set_visible(!reading.profiles.is_empty());
        }
        for (key, button) in &self.prof_buttons {
            if *key == reading.profile {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }

        {
            let mut data = self.chart_data.borrow_mut();
            if let Some(points) = reading.points {
                data.0 = points;
            }
            data.1 = now_pct;
        }
        self.chart.queue_draw();

        if !self.loaded.replace(true) {
            self.popup.show();
        }
        if self.again.replace(false) {
            self.refresh();
        }
    }

    fn on_profile(self: &Rc<Self>, key: &'static str) {
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let ok = gio::spawn_blocking(move || power::set_profile(key)).await.unwrap_or(false);
            if !ok {
                let _ = spawn_detached("notify-send", &["-u", "critical", &tr("Battery"), &tr("Could not change the power profile")]);
            }
            if let Some(p) = me.upgrade() {
                p.refresh();
            }
        });
    }
}

/// Carga nas últimas HOURS horas, em degraus (o upower só grava mudanças). As cores são as do Python.
fn draw_chart(cr: &gtk4::cairo::Context, points: &[(f64, f64)], now_pct: Option<f64>, w: f64, h: f64) {
    let now = glib::real_time() as f64 / 1e6;
    // A grade em 0, 50 e 100%
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
    cr.set_line_width(1.0);
    for v in [0.0, 50.0, 100.0] {
        let y = upower::chart_y(v, h).round() + 0.5;
        cr.move_to(3.0, y);
        cr.line_to(w - 3.0, y);
    }
    let _ = cr.stroke();
    let path = upower::chart_path(points, now_pct, now, w, h);
    let (Some(first), Some(last)) = (path.first(), path.last()) else { return };
    let (r, g, b) = (f64::from(0x19) / 255.0, f64::from(0x9c) / 255.0, f64::from(0xa8) / 255.0);
    // Área
    let bottom = upower::chart_y(0.0, h);
    cr.move_to(first.0, bottom);
    for (x, y) in &path {
        cr.line_to(*x, *y);
    }
    cr.line_to(last.0, bottom);
    cr.close_path();
    cr.set_source_rgba(r, g, b, 0.25);
    let _ = cr.fill();
    // Linha
    cr.move_to(first.0, first.1);
    for (x, y) in &path[1..] {
        cr.line_to(*x, *y);
    }
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(2.0);
    let _ = cr.stroke();
}

/// Avisa o ícone da bateria (waybar-battery watch), que esconde a dica com o popup aberto.
fn notify_icon() {
    resident::signal_pid(&resident::pidfile("waybar-battery-watch"), "waybar-battery", libc::SIGUSR1);
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-battery-popup\n",
    "    Opens the battery popup: charge, time, health, a 12 h graph and the power profiles (opening it again closes it).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-battery-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn health_like_python() {
        assert_eq!(health_text(&fields(&[("energy-full", "49.68 Wh"), ("energy-full-design", "57 Wh")])), "87% of the original capacity");
        assert_eq!(
            health_text(&fields(&[("energy-full", "50 Wh"), ("energy-full-design", "50 Wh"), ("charge-cycles", "1")])),
            "100% of the original capacity · 1 cycle"
        );
        assert_eq!(health_text(&fields(&[("charge-cycles", "120")])), "— · 120 cycles");
        assert_eq!(health_text(&fields(&[("charge-cycles", "N/A")])), "—");
        assert_eq!(health_text(&fields(&[("charge-cycles", "0")])), "—");
    }
}
