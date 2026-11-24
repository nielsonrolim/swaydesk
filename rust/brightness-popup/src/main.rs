//! Popup do brilho para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-brightness-popup`,
//! que passa a vez a este binário (sway_rust).
//!
//! Brilho e luz noturna, com tamanho fixo. O brilho, o próprio popup muda pelo brightnessctl (com o mesmo
//! dispositivo e o mesmo piso do sway-brightness, que é do swaydesk-osd), e a luz noturna é do sway-nightlight:
//! aqui só há o slider e os botões. Clicar de novo no ícone, Esc, clicar fora ou
//! trocar de workspace fecha.
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_ui::cmd::{bin, spawn, spawn_detached, status};
use swaydesk_ui::list::{Options, CSS_OPT, CSS_SCALE};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-brightness-popup";
const ICON: &str = "\u{f00e0}"; // brightness-6
/// Depois de mexer no slider, a leitura do arquivo não o move: o valor gravado ainda pode não ter chegado.
/// Com o botão pressionado no slider (`pressed`), também não
const HANDS_OFF: Duration = Duration::from_millis(1500);
/// Quanto um clique na luz noturna segura o botão até o sway-nightlight gravar o modo novo
const MODE_HOLD: Duration = Duration::from_secs(3);

const CSS: &str = r#"
.icon { font-family: "Symbols Nerd Font"; font-size: 17px; padding: 0 6px; }
.pct { font-size: 12px; min-width: 38px; }
.detail { color: #8a8a8f; font-size: 11px; }
button.opt:disabled { color: #5c5c62; }
.time-word { font-size: 12px; color: #8a8a8f; }
entry {
  background: #38383d; color: #dadada; border: none; box-shadow: none; border-radius: 6px;
  font-size: 12px; min-height: 0; padding: 2px 6px; outline: none;
}
entry:focus-within { box-shadow: inset 0 0 0 1px @accent; outline: none; }
entry.invalid, entry.invalid:focus-within { box-shadow: inset 0 0 0 1px #f27835; }
"#;

/// O primeiro da lista, o mesmo do ícone e do sway-brightness.
fn backlight_dir(root: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    dirs.sort();
    dirs.into_iter().find(|d| {
        ["brightness", "max_brightness"].iter().all(|f| std::fs::File::open(d.join(f)).is_ok())
    })
}

/// Os argumentos do brightnessctl para `sway-brightness set N`: o dispositivo pelo nome, o piso de 1% do
/// máximo (no mínimo 1: em 0 muitos painéis apagam a luz de fundo) e o valor em %, só de 1 a 100.
fn set_args(dir: &Path, value: i64) -> Option<Vec<String>> {
    let max = read_number(&dir.join("max_brightness"))?;
    let device = dir.file_name()?.to_string_lossy().into_owned();
    let floor = (max / 100).max(1);
    (1..=100).contains(&value).then(|| {
        ["-q", "-d", &device, &format!("--min-value={floor}"), "set", &format!("{value}%")].map(String::from).to_vec()
    })
}

fn read_number(path: &Path) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// O brilho em %, arredondado, nunca abaixo de 1 (o piso do sway-brightness).
fn read_pct(dir: &Path) -> Option<i64> {
    let (cur, top) = (read_number(&dir.join("brightness"))?, read_number(&dir.join("max_brightness"))?);
    (top > 0).then(|| ((cur * 100 + top / 2) / top).max(1))
}

fn valid_time(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && matches!((b[0], b[1]), (b'0'..=b'1', b'0'..=b'9') | (b'2', b'0'..=b'3'))
        && matches!((b[3], b[4]), (b'0'..=b'5', b'0'..=b'9'))
}

/// O problema do horário, se houver: (mensagem, [início ruim, fim ruim]).
fn schedule_problem(start: &str, end: &str) -> (Option<String>, [bool; 2]) {
    let bad = [!valid_time(start), !valid_time(end)];
    if bad[0] || bad[1] {
        return (Some(tr("Use the format 18:00")), bad);
    }
    if end >= start {
        return (Some(tr("The end must come before the start on the same day")), bad);
    }
    (None, bad)
}

/// O `sway-nightlight status`: (modo, se o gammastep caiu). "unavailable" sem o gammastep.
fn parse_night(text: &str) -> (String, bool) {
    let mut words = text.split_whitespace();
    let mode = words.next().unwrap_or("off").to_string();
    (mode, words.next() == Some("failed"))
}

/// O `sway-nightlight schedule`, ou o padrão.
fn parse_schedule(text: &str) -> (String, String) {
    match text.split_whitespace().collect::<Vec<_>>().as_slice() {
        [start, end] => (start.to_string(), end.to_string()),
        _ => ("18:00".into(), "06:00".into()),
    }
}

struct Reading {
    night: String,
    schedule: Option<String>,
    gen: u64,
}

struct Popup {
    popup: Rc<PopupWindow>,
    backlight: Option<PathBuf>,
    scale: gtk4::Scale,
    pct: gtk4::Label,
    night: Options,
    schedule_row: gtk4::Box,
    start: gtk4::Entry,
    end: gtk4::Entry,
    night_detail: gtk4::Label,
    /// Mudanças vindas daqui (a leitura), que não são do usuário
    updating: Cell<bool>,
    /// A última mão no slider
    touched: Cell<Option<Instant>>,
    /// O botão do mouse está pressionado no slider (o arrasto do Python)
    pressed: Rc<Cell<bool>>,
    /// O valor que espera a vez de ser gravado
    pending: Cell<Option<i64>>,
    apply_source: RefCell<Option<glib::SourceId>>,
    /// O brightnessctl em andamento (o `sway-brightness set` em Rust): um por vez, para o último valor valer
    writer: RefCell<Option<JoinHandle<()>>>,
    schedule_source: RefCell<Option<glib::SourceId>>,
    /// Fica até a próxima edição: a atualização não pode apagá-lo
    schedule_error: RefCell<Option<String>>,
    reading: Cell<bool>,
    loaded: Cell<bool>,
    /// Muda a cada clique: uma leitura começada antes não desfaz o botão
    gen: Cell<u64>,
    /// (modo clicado, prazo): segura o botão até o comando gravar
    pending_mode: RefCell<Option<(String, Instant)>>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-brightness", &format!("{CSS_SCALE}{CSS_OPT}{CSS}"), true, main_loop);
        let root = std::env::var_os("BACKLIGHT_DIR").map(PathBuf::from).unwrap_or_else(|| "/sys/class/backlight".into());
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        card.add_css_class("card");
        card.set_size_request(340, -1);
        popup.window.set_child(Some(&card));

        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let picker = me.clone();
            let night = Options::new(
                &[
                    ("off", ptr("night light", "Off")),
                    ("on", ptr("night light", "On")),
                    ("auto", tr("Scheduled")),
                ],
                move |key| {
                    if let Some(p) = picker.upgrade() {
                        p.on_night(key);
                    }
                },
            );
            let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 1.0, 100.0, 1.0);
            scale.set_draw_value(false);
            scale.set_hexpand(true);
            Popup {
                popup: popup.clone(),
                backlight: backlight_dir(&root),
                scale,
                pct: label("", &["pct"], 1.0),
                night,
                schedule_row: gtk4::Box::new(gtk4::Orientation::Horizontal, 4),
                start: time_entry(),
                end: time_entry(),
                night_detail: label(" ", &["detail"], 0.0),
                updating: Cell::new(false),
                touched: Cell::new(None),
                pressed: Rc::new(Cell::new(false)),
                pending: Cell::new(None),
                apply_source: RefCell::new(None),
                writer: RefCell::new(None),
                schedule_source: RefCell::new(None),
                schedule_error: RefCell::new(None),
                reading: Cell::new(false),
                loaded: Cell::new(false),
                gen: Cell::new(0),
                pending_mode: RefCell::new(None),
            }
        });

        if this.backlight.is_some() {
            card.append(&label(&tr("Brightness"), &["section"], 0.0));
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            row.append(&label(ICON, &["icon"], 0.5));
            row.append(&this.scale);
            row.append(&this.pct);
            card.append(&row);
            // Só observa o botão, na captura: um GestureClick próprio é cancelado quando o arrasto do
            // Scale reclama a sequência (visto com o vpointer), e a marca cairia com o dedo no slider
            let press = gtk4::EventControllerLegacy::new();
            press.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let pressed = this.pressed.clone();
            press.connect_event(move |_, event| {
                match event.event_type() {
                    gtk4::gdk::EventType::ButtonPress | gtk4::gdk::EventType::TouchBegin => pressed.set(true),
                    gtk4::gdk::EventType::ButtonRelease
                    | gtk4::gdk::EventType::TouchEnd
                    | gtk4::gdk::EventType::TouchCancel
                    | gtk4::gdk::EventType::GrabBroken => {
                        pressed.set(false)
                    }
                    _ => {}
                }
                glib::Propagation::Proceed
            });
            this.scale.add_controller(press);
            let me = Rc::downgrade(&this);
            this.scale.connect_value_changed(move |scale| {
                if let Some(p) = me.upgrade() {
                    p.on_scale(scale.value() as i64);
                }
            });
        }

        card.append(&label(&tr("Night light"), &["section"], 0.0));
        card.append(&this.night.widget);
        // Sempre presente, só some da vista: mudar o tamanho faria o Sway tirar o foco do popup
        this.schedule_row.append(&label(&tr("From"), &["time-word"], 0.5));
        this.schedule_row.append(&this.start);
        this.schedule_row.append(&label(&tr("to"), &["time-word"], 0.5));
        this.schedule_row.append(&this.end);
        card.append(&this.schedule_row);
        card.append(&this.night_detail);
        for entry in [&this.start, &this.end] {
            let me = Rc::downgrade(&this);
            entry.connect_changed(move |_| {
                if let Some(p) = me.upgrade() {
                    p.on_schedule();
                }
            });
            let me = Rc::downgrade(&this);
            entry.connect_activate(move |_| {
                if let Some(p) = me.upgrade() {
                    p.save_schedule();
                }
            });
        }

        this.refresh(); // a janela aparece quando a primeira leitura chegar
        let me = Rc::downgrade(&this);
        glib::timeout_add_seconds_local(2, move || match me.upgrade() {
            Some(p) => {
                p.refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        this
    }

    // --- horário ---
    fn check_schedule(&self) -> Option<String> {
        let (problem, bad) = schedule_problem(&self.start.text(), &self.end.text());
        for (entry, bad) in [(&self.start, bad[0]), (&self.end, bad[1])] {
            if bad {
                entry.add_css_class("invalid");
            } else {
                entry.remove_css_class("invalid");
            }
        }
        problem
    }

    fn on_schedule(self: &Rc<Self>) {
        if self.updating.get() {
            return;
        }
        // A borda laranja acompanha a digitação, como no Python; a mensagem e a gravação esperam
        let _ = self.check_schedule();
        // Espera parar de digitar: gravar reinicia o gammastep
        if let Some(source) = self.schedule_source.take() {
            source.remove();
        }
        let me = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let Some(p) = me.upgrade() {
                p.schedule_source.take();
                p.save_schedule();
            }
        });
        self.schedule_source.replace(Some(source));
    }

    fn save_schedule(self: &Rc<Self>) {
        if let Some(source) = self.schedule_source.take() {
            source.remove();
        }
        self.schedule_error.replace(None);
        let problem = self.check_schedule();
        self.night_detail.set_text(&problem.clone().unwrap_or_else(|| tr("4500 K during these hours")));
        if problem.is_some() {
            return;
        }
        let (start, end) = (self.start.text().to_string(), self.end.text().to_string());
        let me = Rc::downgrade(self);
        // Em outra linha, porque espera o gammastep trocar; um erro precisa aparecer no popup
        glib::MainContext::default().spawn_local(async move {
            let error = gio::spawn_blocking(move || write_schedule(&start, &end)).await.ok().flatten();
            if let (Some(error), Some(p)) = (error, me.upgrade()) {
                p.night_detail.set_text(&error);
                p.schedule_error.replace(Some(error));
            }
        });
    }

    // --- dados ---
    fn refresh(self: &Rc<Self>) {
        if self.reading.replace(true) {
            return;
        }
        let (first, gen) = (!self.loaded.get(), self.gen.get());
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let reading = gio::spawn_blocking(move || Reading {
                night: status(bin("sway-nightlight"), &["status"]),
                schedule: first.then(|| status(bin("sway-nightlight"), &["schedule"])),
                gen,
            })
            .await;
            let Some(p) = me.upgrade() else { return };
            p.reading.set(false); // também depois de um pânico na leitura: senão o popup não relê mais
            if let Ok(reading) = reading {
                p.apply(reading);
            }
        });
    }

    fn apply(&self, reading: Reading) {
        if reading.gen != self.gen.get() {
            return; // leitura que começou antes do clique: não desfaz o botão
        }
        if let Some(schedule) = &reading.schedule {
            let (start, end) = parse_schedule(schedule);
            self.updating.set(true);
            self.start.set_text(&start);
            self.end.set_text(&end);
            self.updating.set(false);
        }

        let hands_off = self.pressed.get() || self.touched.get().is_some_and(|t| t.elapsed() < HANDS_OFF);
        if let (Some(dir), false, None, false) = (&self.backlight, hands_off, self.pending.get(), self.writing()) {
            if let Some(pct) = read_pct(dir) {
                self.updating.set(true);
                self.scale.set_value(pct as f64);
                self.updating.set(false);
                self.pct.set_text(&format!("{pct}%"));
            }
        }

        let (mut mode, mut failed) = parse_night(&reading.night);
        let held = self.pending_mode.borrow().clone();
        if let Some((key, deadline)) = held {
            if mode == key || Instant::now() > deadline {
                self.pending_mode.replace(None);
            } else if mode != "unavailable" {
                mode = key; // o comando ainda não gravou o modo novo: mantém o clique
                failed = false; // e o erro lido era do modo antigo
            }
        }
        if mode == "unavailable" {
            self.night.set_sensitive(false);
            self.night.select("");
            self.schedule_row.set_opacity(0.0);
            self.schedule_row.set_sensitive(false);
            self.night_detail.set_text(&tr("Install gammastep to use it"));
        } else {
            self.night.set_sensitive(true);
            self.night.select(&mode);
            let auto = mode == "auto";
            self.schedule_row.set_opacity(if auto { 1.0 } else { 0.0 });
            self.schedule_row.set_sensitive(auto);
            let mut problem = if auto {
                self.schedule_error.borrow().clone().or_else(|| self.check_schedule())
            } else {
                None
            };
            if failed {
                problem = Some(tr("gammastep is not running; choose the mode again"));
            }
            let text = problem.unwrap_or_else(|| match mode.as_str() {
                "on" => tr("4500 K all the time"),
                "auto" => tr("4500 K during these hours"),
                _ => " ".into(),
            });
            self.night_detail.set_text(&text);
        }
        if !self.loaded.replace(true) {
            self.popup.show();
        }
    }

    // --- brilho ---
    fn on_scale(self: &Rc<Self>, value: i64) {
        self.pct.set_text(&format!("{value}%"));
        if self.updating.get() {
            return;
        }
        self.touched.set(Some(Instant::now()));
        // Agrupa o arrasto: sem isso seria um processo por pixel
        self.pending.set(Some(value));
        if self.apply_source.borrow().is_none() {
            self.schedule_apply(Duration::from_millis(60));
        }
    }

    fn schedule_apply(self: &Rc<Self>, wait: Duration) {
        let me = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(wait, move || {
            if let Some(p) = me.upgrade() {
                p.apply_source.take();
                p.apply_brightness();
            }
        });
        self.apply_source.replace(Some(source));
    }

    fn writing(&self) -> bool {
        self.writer.borrow().as_ref().is_some_and(|writer| !writer.is_finished())
    }

    fn apply_brightness(self: &Rc<Self>) {
        if self.pending.get().is_none() {
            return;
        }
        if self.writing() {
            // Tenta de novo; até lá só o valor mais recente fica guardado
            self.schedule_apply(Duration::from_millis(30));
            return;
        }
        if let Some(value) = self.pending.take() {
            self.set_brightness(value);
        }
    }

    fn set_brightness(&self, value: i64) {
        // O `sway-brightness set` em Rust: o mesmo dispositivo e o piso, e o ícone avisado depois do
        // brightnessctl, numa linha à parte (o GTK não espera)
        let Some(args) = self.backlight.as_deref().and_then(|dir| set_args(dir, value)) else { return };
        let writer = std::thread::spawn(move || {
            if let Err(err) = Command::new("brightnessctl").args(&args).status() {
                // Uma vez por processo: o slider tenta a cada movimento
                static WARNED: std::sync::Once = std::sync::Once::new();
                WARNED.call_once(|| eprintln!("{PROGRAM}: could not run brightnessctl: {err} (install the brightnessctl package)"));
            }
            notify_icon();
        });
        // O anterior já terminou (o writing() é conferido antes): o join não espera
        if let Some(old) = self.writer.replace(Some(writer)) {
            let _ = old.join();
        }
    }

    /// Espera a escrita em curso, até `limit`.
    fn wait_writer(&self, limit: Duration) {
        let deadline = Instant::now() + limit;
        while self.writing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // --- luz noturna ---
    fn on_night(self: &Rc<Self>, key: &str) {
        // O botão responde na hora; o gammastep troca a cor depois (e pode levar o fade)
        self.night.select(key);
        self.gen.set(self.gen.get() + 1);
        self.pending_mode.replace(Some((key.to_string(), Instant::now() + MODE_HOLD)));
        let _ = spawn_detached(bin("sway-nightlight"), &[key]);
        self.refresh_soon(Duration::from_millis(200));
    }

    /// Relê uma vez; se houver leitura em curso, que pode ter começado antes do clique, tenta já já.
    fn refresh_soon(self: &Rc<Self>, wait: Duration) {
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(wait, move || {
            let Some(p) = me.upgrade() else { return };
            if p.reading.get() {
                p.refresh_soon(Duration::from_millis(100));
            } else {
                p.refresh();
            }
        });
    }
}

/// Ao fechar: o que ainda está esperando não pode se perder.
impl Drop for Popup {
    fn drop(&mut self) {
        if let Some(source) = self.schedule_source.take() {
            source.remove();
            if schedule_problem(&self.start.text(), &self.end.text()).0.is_none() {
                let _ = spawn(bin("sway-nightlight"), &["schedule", &self.start.text(), &self.end.text()]);
            }
        }
        if let Some(value) = self.pending.take() {
            // A espera abaixo é sem o laço do GTK: a janela some antes, em vez de ficar congelada
            self.popup.window.set_visible(false);
            if let Some(display) = gtk4::gdk::Display::default() {
                display.flush();
            }
            self.wait_writer(Duration::from_secs(1));
            self.set_brightness(value);
            // A linha da escrita morre com o processo: espera o brightnessctl e o aviso ao ícone
            self.wait_writer(Duration::from_secs(1));
        }
    }
}

fn time_entry() -> gtk4::Entry {
    let entry = gtk4::Entry::new();
    entry.set_width_chars(5);
    entry.set_max_width_chars(5);
    entry.set_max_length(5);
    EntryExt::set_alignment(&entry, 0.5);
    entry
}

/// Grava o horário; devolve o erro para o popup, se houver.
fn write_schedule(start: &str, end: &str) -> Option<String> {
    let failed = || tr("Could not save the schedule");
    let out = std::process::Command::new("timeout")
        .arg("10")
        .arg(bin("sway-nightlight"))
        .args(["schedule", start, end])
        .stdin(std::process::Stdio::null())
        .output();
    let out = match out {
        Ok(out) => out,
        Err(_) => return Some(failed()),
    };
    if out.status.success() {
        return None;
    }
    let msg = String::from_utf8_lossy(&out.stderr);
    let msg = msg.trim();
    let msg = msg.strip_prefix("sway-nightlight: ").unwrap_or(msg);
    Some(if msg.is_empty() { failed() } else { msg.chars().take(60).collect() })
}

fn notify_icon() {
    // O ícone esconde a dica com o popup aberto
    swaydesk_common::resident::notify_icons("waybar-brightness");
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-brightness-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_schedule() {
        for good in ["00:00", "18:00", "23:59", "09:05"] {
            assert!(valid_time(good), "{good}");
        }
        for bad in ["24:00", "7:00", "18:60", "18-00", "", "18:000"] {
            assert!(!valid_time(bad), "{bad}");
        }
        assert_eq!(schedule_problem("18:00", "06:00"), (None, [false, false]));
        assert_eq!(schedule_problem("18:00", "6:00").1, [false, true]);
        assert!(schedule_problem("06:00", "18:00").0.is_some()); // o fim vem antes do início, no outro dia
        assert!(schedule_problem("18:00", "18:00").0.is_some());
    }

    #[test]
    fn night_and_schedule_lines() {
        assert_eq!(parse_night("auto failed"), ("auto".into(), true));
        assert_eq!(parse_night("on"), ("on".into(), false));
        assert_eq!(parse_night(""), ("off".into(), false));
        assert_eq!(parse_schedule("19:30 05:45"), ("19:30".into(), "05:45".into()));
        assert_eq!(parse_schedule(""), ("18:00".into(), "06:00".into()));
    }

    /// Os mesmos argumentos do brightnessctl que o script home/.local/bin/sway-brightness passa no `set N`
    /// (com um brightnessctl falso que grava o que recebe), e os mesmos valores recusados.
    #[test]
    fn set_args_match_the_sway_brightness_script() {
        use std::os::unix::fs::PermissionsExt;
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/bin/sway-brightness");
        let dir = std::env::temp_dir().join(format!("swaydesk-brightness-set-{}", std::process::id()));
        let (root, fake, run) = (dir.join("backlight"), dir.join("bin"), dir.join("run"));
        for d in [&root, &fake, &run] {
            std::fs::create_dir_all(d).unwrap();
        }
        let calls = dir.join("calls");
        let ctl = fake.join("brightnessctl");
        std::fs::write(&ctl, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", calls.display())).unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(root.join("a_without_files")).unwrap();
        let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap_or_default());
        for max in [937, 50, 120000] {
            let dev = root.join("intel_backlight");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("max_brightness"), format!("{max}\n")).unwrap();
            std::fs::write(dev.join("brightness"), "10\n").unwrap();
            assert_eq!(backlight_dir(&root), Some(dev.clone()));
            for value in ["1", "42", "100", "0", "101", "-3"] {
                let _ = std::fs::remove_file(&calls);
                let out = std::process::Command::new("bash")
                    .arg(&script)
                    .args(["set", value])
                    .env("BACKLIGHT_DIR", &root)
                    .env("PATH", &path)
                    .env("XDG_RUNTIME_DIR", &run)
                    .output()
                    .unwrap();
                let from_script = std::fs::read_to_string(&calls).ok().map(|t| t.trim_end().to_string());
                let ours = set_args(&dev, value.parse().unwrap()).map(|a| a.join(" "));
                assert_eq!(ours, from_script, "max {max}, set {value}: {}", String::from_utf8_lossy(&out.stderr));
                assert_eq!(ours.is_some(), out.status.success(), "max {max}, set {value}");
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn percent_like_python() {
        let dir = std::env::temp_dir().join(format!("swaydesk-brightness-{}", std::process::id()));
        let dev = dir.join("b_dev");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::create_dir_all(dir.join("a_without_files")).unwrap();
        std::fs::write(dev.join("max_brightness"), "937\n").unwrap();
        for (raw, pct) in [(937, 100), (468, 50), (0, 1), (5, 1), (14, 1), (15, 2)] {
            std::fs::write(dev.join("brightness"), format!("{raw}\n")).unwrap();
            // (cur * 100 + top // 2) // top, com o piso 1
            assert_eq!(read_pct(&dev), Some(pct), "{raw}");
        }
        assert_eq!(backlight_dir(&dir), Some(dev.clone()));
        std::fs::write(dev.join("max_brightness"), "0").unwrap();
        assert_eq!(read_pct(&dev), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
