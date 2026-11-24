//! Popup do calendário para o clique no relógio da Waybar: o porte de `home/.local/bin/waybar-calendar`
//! para GTK 4, sobre o `swaydesk-ui`.
//!
//! O mês na língua da interface, com a semana começando no domingo, hoje na cor de destaque e os
//! feriados marcados e listados. Os feriados vêm da python3-holidays, pelo `waybar_holidays.py`
//! (a mesma lógica da versão em Python), em outra linha. ‹ › e a rolagem trocam o mês; "Hoje" volta.
//! Ao abrir e ao fechar, avisa o relógio da Waybar (SIGUSR1), que esconde a dica.
mod dates;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;

use dates::{month_grid, shift_month, Date};
use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_common::{config, resident};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-calendar";

const CSS: &str = r#"
.title  { font-size: 16px; font-weight: bold; color: #ffffff; }
.box    { background: #38383d; border-radius: 10px; padding: 8px 10px; }
.wd     { color: #8a8a8f; font-size: 11px; }
.day    { font-size: 13px; min-width: 28px; min-height: 28px; border-radius: 14px; }
.other  { color: #5c5c62; }
.weekend { color: #b0b0b5; }
.holiday { color: #f27835; font-weight: bold; }
.optional { color: #f27835; }
.today  { background: @accent; color: @on_accent; font-weight: bold; }
.hol-date { color: #f27835; font-size: 12px; }
.hol-name { font-size: 12px; }
.foot   { color: #8a8a8f; font-size: 12px; }
button.nav {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  padding: 2px 10px; min-height: 0; border-radius: 8px; font-size: 16px;
}
button.nav:hover { background: #4c4c50; }
"#;

fn month_name(month: u32) -> String {
    let names = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
                 "November", "December"];
    ptr("month", names[month as usize - 1])
}

/// Da grade, que começa no domingo.
fn weekday_short(column: usize) -> String {
    ptr("calendar weekday", ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][column])
}

/// Com a segunda em 0, como o `date.weekday()` do Python.
fn weekday_long(index: usize) -> String {
    ptr("weekday", ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][index])
}

fn today() -> Date {
    let now = glib::DateTime::now_local().expect("a hora local");
    Date::new(now.year(), now.month() as u32, now.day_of_month() as u32)
}

/// Os feriados de um ano e dos vizinhos, como o `waybar_holidays.shown`.
#[derive(Clone, Default)]
struct Holidays {
    days: HashMap<Date, (String, bool)>,
    note: Option<String>,
    listed: bool,
}

fn parse_holidays(text: &str) -> Option<Holidays> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut days = HashMap::new();
    for item in value.get("holidays")?.as_array()? {
        let item = item.as_array()?;
        let date = Date::parse_iso(item.first()?.as_str()?)?;
        days.insert(date, (item.get(1)?.as_str()?.to_string(), item.get(2)?.as_bool()?));
    }
    Some(Holidays {
        days,
        note: value.get("note").and_then(|n| n.as_str()).map(str::to_string),
        listed: value.get("listed")?.as_bool()?,
    })
}

/// Pela versão em Python da lógica (a python3-holidays não existe em Rust). Fora da linha principal.
fn fetch_holidays(year: i32) -> Option<Holidays> {
    fetch_holidays_with("python3", year)
}

/// O mesmo, com o interpretador dado. Sem ele (no Arch, o python é só um optdepends do swaydesk-calendar), o
/// None: o calendário abre sem a caixa dos feriados, sem aviso na tela.
fn fetch_holidays_with(python: &str, year: i32) -> Option<Holidays> {
    let script = swaydesk_common::paths::helper("waybar_holidays.py");
    let output = Command::new(python).arg(&script).arg(year.to_string()).output().ok()?;
    if !output.status.success() {
        eprintln!("{PROGRAM}: the holidays failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        return None;
    }
    parse_holidays(&String::from_utf8_lossy(&output.stdout))
}

/// Onde o python3 procura os módulos: o PYTHONPATH e os site-packages/dist-packages do sistema, do
/// /usr/local e do ~/.local (python3/ do Debian, python3.14/ do Arch).
fn python_module_dirs(home: Option<&Path>, pythonpath: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = pythonpath.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let mut bases = vec![PathBuf::from("/usr/lib"), PathBuf::from("/usr/lib64"), PathBuf::from("/usr/local/lib")];
    bases.extend(home.map(|h| h.join(".local/lib")));
    for base in bases {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("python3"))
            .flat_map(|e| [e.path().join("site-packages"), e.path().join("dist-packages")])
            .collect();
        found.sort();
        dirs.extend(found);
    }
    dirs
}

/// Se a python3-holidays está instalada, sem rodar o Python: decide, antes de a janela abrir, se o lugar
/// da caixa dos feriados fica reservado. Sem isso, com o país do locale e sem a biblioteca, o popup abria
/// com a caixa vazia e encolhia quando a resposta chegava.
fn holidays_module_in(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| d.join("holidays/__init__.py").is_file())
}

struct Calendar {
    popup: Rc<PopupWindow>,
    title: gtk4::Label,
    body: gtk4::Box,
    back: gtk4::Button,
    year: Cell<i32>,
    month: Cell<u32>,
    /// Se há país na config: sem ele, nunca há caixa de feriados
    country: bool,
    /// Se o lugar da caixa fica reservado enquanto os feriados não chegam: o país veio da config (a caixa
    /// aparece, nem que seja com o aviso de instalar a biblioteca) ou a python3-holidays está instalada
    expect_box: bool,
    holidays: RefCell<HashMap<i32, Holidays>>,
    fetching: RefCell<Vec<i32>>,
    /// Os anos cuja leitura falhou: sem caixa até a próxima troca de mês, que tenta de novo
    failed: RefCell<Vec<i32>>,
}

impl Calendar {
    fn new(main_loop: &glib::MainLoop) -> Rc<Calendar> {
        let popup = PopupWindow::new(PROGRAM, "waybar-calendar", CSS, false, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(320, -1);
        popup.window.set_child(Some(&card));

        // Cabeçalho e rodapé são montados uma vez só: só o miolo (grade e feriados) é refeito a cada mês
        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let prev = gtk4::Button::with_label("‹");
        prev.add_css_class("nav");
        let next = gtk4::Button::with_label("›");
        next.add_css_class("nav");
        let title = label("", &["title"], 0.5);
        title.set_hexpand(true);
        head.append(&prev);
        head.append(&title);
        head.append(&next);
        card.append(&head);

        let body = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.append(&body);

        let t = today();
        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let text = tr("%(weekday)s, %(month)s %(day)d")
            .replace("%(weekday)s", &weekday_long(t.weekday_from_monday()))
            .replace("%(month)s", &month_name(t.month))
            .replace("%(day)d", &t.day.to_string());
        foot.append(&label(&text, &["foot"], 0.0));
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        let back = gtk4::Button::with_label(&tr("Today")); // só aparece fora do mês atual
        back.add_css_class("link");
        foot.append(&back);
        card.append(&foot);

        // O da config ou, sem ele, o do locale (o waybar_holidays.py faz a mesma conta)
        let (country, explicit) = config::holidays_country(&config::load(), &std::env::vars().collect());
        let country = !country.is_empty();
        let expect_box = country && (explicit || {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            holidays_module_in(&python_module_dirs(home.as_deref(), std::env::var_os("PYTHONPATH").as_deref()))
        });
        let calendar = Rc::new(Calendar {
            popup,
            title,
            body,
            back,
            year: Cell::new(t.year),
            month: Cell::new(t.month),
            country,
            expect_box,
            holidays: RefCell::new(HashMap::new()),
            fetching: RefCell::new(Vec::new()),
            failed: RefCell::new(Vec::new()),
        });
        // Referências fracas: o Calendar é dono da janela que tem os botões, e uma forte faria um ciclo
        // que nunca solta a janela nem os click-catchers
        let c = Rc::downgrade(&calendar);
        prev.connect_clicked(move |_| {
            if let Some(c) = c.upgrade() {
                c.shift(-1);
            }
        });
        let c = Rc::downgrade(&calendar);
        next.connect_clicked(move |_| {
            if let Some(c) = c.upgrade() {
                c.shift(1);
            }
        });
        let c = Rc::downgrade(&calendar);
        calendar.back.connect_clicked(move |_| {
            if let Some(c) = c.upgrade() {
                c.go_today();
            }
        });

        let keys = gtk4::EventControllerKey::new();
        let c = Rc::downgrade(&calendar);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(c) = c.upgrade() else { return glib::Propagation::Proceed };
            use gtk4::gdk::Key;
            match key {
                Key::Left | Key::Page_Up => c.shift(-1),
                Key::Right | Key::Page_Down => c.shift(1),
                Key::Home | Key::t => c.go_today(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        calendar.popup.window.add_controller(keys);
        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let c = Rc::downgrade(&calendar);
        scroll.connect_scroll(move |_, _dx, dy| {
            if let Some(c) = c.upgrade() {
                if dy.abs() >= 0.5 {
                    c.shift(if dy > 0.0 { 1 } else { -1 });
                }
            }
            glib::Propagation::Stop
        });
        card.add_controller(scroll);

        calendar.render();
        calendar.popup.show();
        calendar
    }

    fn shift(self: &Rc<Self>, n: i32) {
        self.failed.borrow_mut().clear(); // navegar tenta de novo os feriados que falharam
        let (year, month) = shift_month(self.year.get(), self.month.get(), n);
        self.year.set(year);
        self.month.set(month);
        self.render();
    }

    fn go_today(self: &Rc<Self>) {
        self.failed.borrow_mut().clear();
        let t = today();
        self.year.set(t.year);
        self.month.set(t.month);
        self.render();
    }

    /// Os feriados do ano, se já chegaram; senão, pede (uma vez) e redesenha quando chegarem.
    fn holidays_for(self: &Rc<Self>, year: i32) -> Option<Holidays> {
        if !self.country {
            return Some(Holidays::default());
        }
        if let Some(found) = self.holidays.borrow().get(&year) {
            return Some(found.clone());
        }
        if self.failed.borrow().contains(&year) {
            // Sem a caixa: "nenhum feriado neste mês" seria uma informação falsa
            return Some(Holidays::default());
        }
        if !self.fetching.borrow().contains(&year) {
            self.fetching.borrow_mut().push(year);
            let c = Rc::downgrade(self);
            glib::MainContext::default().spawn_local(async move {
                let found = gio::spawn_blocking(move || fetch_holidays(year)).await.ok().flatten();
                let Some(c) = c.upgrade() else { return };
                c.fetching.borrow_mut().retain(|y| *y != year);
                match found {
                    Some(found) => {
                        c.holidays.borrow_mut().insert(year, found);
                    }
                    None => c.failed.borrow_mut().push(year), // a falha não fica guardada para sempre
                }
                if c.year.get() == year {
                    c.render();
                }
            });
        }
        None
    }

    fn render(self: &Rc<Self>) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let (year, month) = (self.year.get(), self.month.get());
        let today = today();
        // Até chegarem, a caixa fica no lugar (vazia), para o tamanho não mudar quando chegarem; sem a
        // biblioteca e com o país do locale, nem aparece (a resposta só a esconderia)
        let pending = self.holidays_for(year);
        let hol = pending.clone().unwrap_or(Holidays { listed: self.expect_box, ..Holidays::default() });

        self.title.set_text(&format!("{} {year}", month_name(month)));

        // Grade: semana começando no domingo, sempre 6 linhas
        let grid = gtk4::Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_row_spacing(2);
        grid.set_column_spacing(2);
        grid.add_css_class("box");
        for column in 0..7 {
            grid.attach(&label(&weekday_short(column), &["wd"], 0.5), column as i32, 0, 1, 1);
        }
        for (row, week) in month_grid(year, month).iter().enumerate() {
            for (column, day) in week.iter().enumerate() {
                let mut classes = vec!["day"];
                let holiday = hol.days.get(day);
                if day.month != month {
                    classes.push("other");
                } else if *day == today {
                    classes.push("today");
                } else if let Some((_, optional)) = holiday {
                    classes.push(if *optional { "optional" } else { "holiday" });
                } else if column == 0 || column == 6 {
                    classes.push("weekend");
                }
                let cell = label(&day.day.to_string(), &classes, 0.5);
                cell.set_halign(gtk4::Align::Center); // destaque de hoje redondo, não esticado
                if let Some((name, optional)) = holiday {
                    let tip = if *optional { tr("%s (optional holiday)").replace("%s", name) } else { name.clone() };
                    cell.set_tooltip_text(Some(&tip));
                }
                grid.attach(&cell, column as i32, row as i32 + 1, 1, 1);
            }
        }
        self.body.append(&grid);

        // Feriados do mês: sempre 3 linhas, para o popup manter o tamanho
        if hol.listed {
            let mut month_hol: Vec<(&Date, &(String, bool))> =
                hol.days.iter().filter(|(d, _)| d.year == year && d.month == month).collect();
            month_hol.sort();
            let list = gtk4::Grid::new();
            list.set_column_spacing(12);
            list.set_row_spacing(4);
            list.add_css_class("box");
            if month_hol.is_empty() {
                let text = if pending.is_none() { " ".to_string() } else { hol.note.clone().unwrap_or_else(|| tr("No holidays this month")) };
                let empty = label(&text, &["hol-name", "wd"], 0.0);
                empty.set_wrap(true);
                empty.set_max_width_chars(40);
                list.attach(&empty, 0, 0, 2, 1);
            }
            for row in month_hol.len()..3 {
                list.attach(&label(" ", &["hol-name"], 0.0), 0, row as i32, 1, 1);
            }
            for (row, (day, (name, optional))) in month_hol.iter().enumerate() {
                let when = format!("{:02} · {}", day.day, weekday_short(day.weekday_from_sunday()));
                list.attach(&label(&when, &["hol-date"], 0.0), 0, row as i32, 1, 1);
                let text = if *optional { tr("%s (optional)").replace("%s", name) } else { name.clone() };
                let name = label(&text, &["hol-name"], 0.0);
                name.set_ellipsize(pango::EllipsizeMode::End);
                name.set_hexpand(true);
                list.attach(&name, 1, row as i32, 1, 1);
            }
            self.body.append(&list);
        }

        let away = (year, month) != (today.year, today.month);
        self.back.set_opacity(if away { 1.0 } else { 0.0 }); // opacidade, não visible: o tamanho não muda
        self.back.set_sensitive(away);
        // A caixa dos feriados pode sumir (a python3-holidays antiga ou ausente, com o país do locale): o
        // focus-out do resize não fecha
        self.popup.rendered_at.set(std::time::Instant::now());
        if self.country && !hol.listed {
            // O lugar da caixa ficou reservado até a resposta: sem isso, a janela não encolhe
            self.popup.window.set_default_size(1, 1);
        }
    }
}

fn refresh_clock() {
    // O relógio (waybar-clock) esconde a dica com o calendário aberto
    resident::signal_pid(&resident::pidfile("waybar-clock"), "waybar-clock", libc::SIGUSR1);
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-calendar em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Calendar::new(main_loop)), refresh_clock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holidays_json_from_python() {
        let text = r#"{"listed": true, "note": null, "holidays": [["2026-12-25", "Natal", false], ["2026-02-16", "Carnaval", true]]}"#;
        let found = parse_holidays(text).unwrap();
        assert!(found.listed);
        assert_eq!(found.days[&Date::new(2026, 2, 16)], ("Carnaval".to_string(), true));
        let none = parse_holidays(r#"{"listed": false, "note": "x", "holidays": []}"#).unwrap();
        assert_eq!((none.listed, none.note.as_deref()), (false, Some("x")));
        assert!(parse_holidays("{").is_none());
    }

    #[test]
    fn without_python_there_are_no_holidays() {
        // O swaydesk-calendar do Arch sem o python (optdepends): nenhum feriado, e nada de pânico nem de erro
        assert!(fetch_holidays_with("swaydesk-no-such-python-for-this-test", 2026).is_none());
    }

    #[test]
    fn holidays_module_found_where_python_looks() {
        let tmp = std::env::temp_dir().join(format!("swaydesk-calendar-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let home = tmp.join("home");
        let extra = tmp.join("extra");
        std::fs::create_dir_all(home.join(".local/lib/python3.14/site-packages")).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        let pythonpath = std::env::join_paths([&extra]).unwrap();
        let dirs = python_module_dirs(Some(&home), Some(&pythonpath));
        assert_eq!(dirs[0], extra);
        assert!(dirs.contains(&home.join(".local/lib/python3.14/site-packages")));
        assert!(!holidays_module_in(&[extra.clone(), home.join(".local/lib/python3.14/site-packages")]));
        // Um diretório holidays sem __init__.py não é o módulo
        std::fs::create_dir_all(home.join(".local/lib/python3.14/site-packages/holidays")).unwrap();
        assert!(!holidays_module_in(&dirs));
        std::fs::write(home.join(".local/lib/python3.14/site-packages/holidays/__init__.py"), "").unwrap();
        assert!(holidays_module_in(&python_module_dirs(Some(&home), None)));
        std::fs::create_dir_all(extra.join("holidays")).unwrap();
        std::fs::write(extra.join("holidays/__init__.py"), "").unwrap();
        assert!(holidays_module_in(&[extra]));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
