//! Alt+Tab em ordem de uso, com um seletor na tela: o substituto do swayr. O porte do `sway-switcher` em
//! Python (GTK 3), que segue valendo onde não há o gtk4-layer-shell.
//!
//! uso: sway-switcher daemon   o residente (o laço do config do Sway o abre): guarda a ordem de uso
//!      sway-switcher next     Alt+Tab: a janela usada antes; de novo, a seguinte
//!      sway-switcher prev     Alt+Shift+Tab: a mais antiga; de novo, a anterior
//!      sway-switcher --check  só confere que o binário carrega
//!
//! O next e o prev avisam o residente (SIGUSR1 e SIGUSR2 ao PID de sway-switcher.pid) e saem. Ele abre um
//! cartão no centro da tela, com o ícone, o título e o workspace de cada janela; soltar o Alt foca a
//! escolhida, o Esc cancela. Um toque rápido não mostra o cartão: ele só aparece se o Alt continuar
//! pressionado depois de SHOW_DELAY, e até lá fica transparente, só para receber o teclado e ver o Alt ser
//! solto. Sem o residente, o next e o prev andam pela árvore, sem cartão.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use serde_json::Value;
use swaydesk_common::{config, i18n, resident};
use swaydesk_dock_logic as dl;
use swaydesk_switcher_logic::{self as sl, Command as Cmd, Order, Window};

const PROGRAM: &str = "sway-switcher";
const ICON_SIZE: i32 = 32;

const CSS: &str = r#"
window { background: transparent; }
.switcher {
  background: rgba(46, 46, 51, 0.97);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 8px;
  min-width: 460px;
  color: #e8e8e8;
}
.switcher-row { padding: 6px 10px; border-radius: 8px; }
.switcher-row.selected { background: @accent; color: @on_accent; }
.switcher-title { font-size: 13px; }
.switcher-workspace {
  font-size: 11px; color: #8a8a8f; background: rgba(255, 255, 255, 0.06);
  border-radius: 999px; padding: 1px 8px;
}
.switcher-row.selected .switcher-workspace { color: @on_accent; background: rgba(0, 0, 0, 0.15); }
"#;

fn warn(message: &str) {
    eprintln!("{PROGRAM}: {message}"); // vai para o sway.log, junto com o resto da sessão
}

/// Os prazos, em ms; as variáveis só para os testes (SWAY_SWITCHER_SHOW_MS, SWAY_SWITCHER_FOCUS_MS).
fn delay(var: &str, default: u64) -> Duration {
    Duration::from_millis(std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default))
}

/// A árvore do Sway, ou None.
fn sway_tree() -> Option<Value> {
    let out = swaydesk_ui::cmd::child_env(&mut Command::new("timeout"))
        .args(["-k", "1", "3", "swaymsg", "-t", "get_tree"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Foca a janela pelo id do Sway (que troca de workspace se preciso). Sem esperar.
fn focus(id: i64) {
    let _ = swaydesk_ui::cmd::spawn_detached("swaymsg", &["-q", &format!("[con_id={id}] focus")]);
}

fn parse_command(text: &str) -> Option<Cmd> {
    match text {
        "next" => Some(Cmd::Next),
        "prev" => Some(Cmd::Prev),
        _ => None,
    }
}

/// next/prev: avisa o residente; sem ele, troca direto pela árvore. Sempre 0: o atalho não tem a quem
/// mostrar um erro.
fn send(command: Cmd) -> ExitCode {
    let signal = if command == Cmd::Next { libc::SIGUSR1 } else { libc::SIGUSR2 };
    if resident::signal_pid(&resident::pidfile(PROGRAM), PROGRAM, signal) {
        return ExitCode::SUCCESS;
    }
    if let Some(target) = sway_tree().and_then(|tree| sl::without_daemon_target(&tree, command)) {
        // Aqui o processo sai logo: espera o swaymsg, em vez de deixá-lo para trás
        let _ = swaydesk_ui::cmd::child_env(&mut Command::new("swaymsg"))
            .args(["-q", &format!("[con_id={target}] focus")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    ExitCode::SUCCESS
}

/// Roda de novo com o nome `sway-switcher` no argv[0], que é o que o /proc mostra a quem confere o arquivo
/// de PID (o next e o prev). Só volta se não conseguir.
fn reexec_as_program() {
    let Ok(exe) = std::env::current_exe() else { return };
    let err = Command::new(exe).arg0(PROGRAM).args(std::env::args_os().skip(1)).exec();
    warn(&format!("could not run as {PROGRAM}: {err}"));
}

// --- Os apps instalados, para os ícones ---

#[derive(Default)]
struct Apps {
    entries: RefCell<Option<Vec<dl::AppEntry>>>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl Apps {
    fn resolve(&self, app_id: &str) -> Option<String> {
        if let Some(found) = self.cache.borrow().get(app_id) {
            return found.clone();
        }
        let mut entries = self.entries.borrow_mut();
        let entries = entries.get_or_insert_with(|| {
            gio::AppInfo::all()
                .into_iter()
                .filter_map(|info| info.downcast::<gio::DesktopAppInfo>().ok())
                .filter_map(|info| {
                    let id = info.id()?.to_string();
                    Some(dl::AppEntry {
                        id: id.strip_suffix(".desktop").unwrap_or(&id).to_string(),
                        wm_class: info.startup_wm_class().map(|c| c.to_string()).unwrap_or_default(),
                    })
                })
                .collect()
        });
        let found = dl::match_app(app_id, entries);
        self.cache.borrow_mut().insert(app_id.to_string(), found.clone());
        found
    }

    /// O ícone do .desktop do app; sem ele, o nome no tema; sem nada, o genérico.
    fn icon(&self, app_id: &str) -> gtk4::Image {
        let theme = gdk::Display::default().map(|d| gtk4::IconTheme::for_display(&d));
        let info = self.resolve(app_id).and_then(|key| gio::DesktopAppInfo::new(&format!("{key}.desktop")));
        let image = match info.and_then(|i| i.icon()) {
            Some(icon) => gtk4::Image::from_gicon(&icon),
            None => {
                let name = [app_id.to_string(), app_id.to_lowercase()]
                    .into_iter()
                    .find(|name| !name.is_empty() && theme.as_ref().is_some_and(|t| t.has_icon(name)))
                    .unwrap_or_else(|| "application-x-executable".into());
                gtk4::Image::from_icon_name(&name)
            }
        };
        image.set_pixel_size(ICON_SIZE);
        image
    }
}

// --- O cartão ---

struct Switcher {
    window: gtk4::Window,
    card: gtk4::Box,
    order: RefCell<Order>,
    windows: RefCell<Vec<Window>>,
    rows: RefCell<Vec<gtk4::Box>>,
    selected: Cell<usize>,
    active: Cell<bool>,
    /// Cada abertura ganha um número: um prazo de uma abertura anterior não age na atual
    generation: Cell<u64>,
    apps: Apps,
    provider: gtk4::CssProvider,
    /// O CSS carregado: o residente vive a sessão toda, e a cor de destaque pode mudar nesse tempo
    css: RefCell<String>,
}

/// A tecla do modificador segurado: o Alt, ou o Super no perfil de atalhos dwm (lá os cartões ficam no Super+Tab).
fn is_alt(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Alt_L | gdk::Key::Alt_R | gdk::Key::Meta_L | gdk::Key::Meta_R | gdk::Key::ISO_Level3_Shift
            | gdk::Key::Super_L | gdk::Key::Super_R
    )
}

/// Os modificadores que seguram o cartão aberto: o Alt e o Super (o dwm).
fn held(state: gdk::ModifierType) -> bool {
    state.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK)
}

/// As teclas que andam com o cartão aberto. O Tab com o Alt segurado nem chega aqui: o bindsym Alt+Tab do
/// Sway o consome e chama o `sway-switcher next`, que cai no ramo do cartão aberto do command()
fn key_move(key: gdk::Key) -> Option<Cmd> {
    match key {
        gdk::Key::Tab | gdk::Key::Down | gdk::Key::Right => Some(Cmd::Next),
        gdk::Key::ISO_Left_Tab | gdk::Key::Up | gdk::Key::Left => Some(Cmd::Prev),
        _ => None,
    }
}

impl Switcher {
    /// Relê a cor de destaque e a fonte: o tema não reinicia o residente, que perderia a ordem de uso.
    /// Só recarrega o provider se o CSS mudou.
    fn refresh_theme(&self) {
        let css = format!("{}{CSS}", swaydesk_ui::theme_css());
        if *self.css.borrow() != css {
            self.provider.load_from_string(&css);
            *self.css.borrow_mut() = css;
        }
    }

    fn new(order: Order) -> Rc<Switcher> {
        let provider = gtk4::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let window = gtk4::Window::new();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_namespace(PROGRAM);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        card.add_css_class("switcher");
        window.set_child(Some(&card));
        let me = Rc::new(Switcher {
            window,
            card,
            order: RefCell::new(order),
            windows: RefCell::default(),
            rows: RefCell::default(),
            selected: Cell::new(0),
            active: Cell::new(false),
            generation: Cell::new(0),
            apps: Apps::default(),
            provider,
            css: RefCell::default(),
        });
        me.refresh_theme();
        let keys = gtk4::EventControllerKey::new();
        let weak = Rc::downgrade(&me);
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(me) = weak.upgrade() {
                me.key_pressed(key);
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&me);
        keys.connect_key_released(move |_, key, _, _| {
            if let Some(me) = weak.upgrade() {
                if me.active.get() && is_alt(key) {
                    me.commit();
                }
            }
        });
        me.window.add_controller(keys);
        // O Alt já foi solto antes de o cartão receber o teclado: o toque rápido. No Wayland, o enter do
        // teclado chega antes dos modificadores: no is-active, o estado ainda pode ser o antigo (com o Alt
        // de um ciclo anterior). Por isso também a mudança dos modificadores: o que chegar por último decide
        let weak = Rc::downgrade(&me);
        me.window.connect_is_active_notify(move |window| {
            if let Some(me) = weak.upgrade() {
                if me.active.get() && window.is_active() && !me.alt_down() {
                    me.commit();
                }
            }
        });
        if let Some(keyboard) = gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.keyboard()) {
            let weak = Rc::downgrade(&me);
            keyboard.connect_modifier_state_notify(move |keyboard| {
                if let Some(me) = weak.upgrade() {
                    if me.active.get() && !held(keyboard.modifier_state()) {
                        me.commit();
                    }
                }
            });
        }
        me
    }

    fn alt_down(&self) -> bool {
        let keyboard = gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.keyboard());
        keyboard.is_some_and(|k| held(k.modifier_state()))
    }

    fn on_event(&self, event: &Value) {
        self.order.borrow_mut().apply_event(event);
    }

    fn fill(self: &Rc<Self>) {
        while let Some(child) = self.card.first_child() {
            self.card.remove(&child);
        }
        let mut rows = Vec::new();
        for (i, window) in self.windows.borrow().iter().enumerate() {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            row.add_css_class("switcher-row");
            row.append(&self.apps.icon(&window.app_id));
            let text = if window.title.is_empty() { &window.app_id } else { &window.title };
            let title = gtk4::Label::new(Some(&sl::short_title(text)));
            title.set_xalign(0.0);
            title.set_hexpand(true);
            title.add_css_class("switcher-title");
            row.append(&title);
            let workspace = gtk4::Label::new(Some(&window.workspace));
            workspace.add_css_class("switcher-workspace");
            row.append(&workspace);
            let click = gtk4::GestureClick::new();
            let weak = Rc::downgrade(self);
            click.connect_released(move |_, _, _, _| {
                if let Some(me) = weak.upgrade() {
                    if me.active.get() {
                        me.selected.set(i);
                        me.commit();
                    }
                }
            });
            row.add_controller(click);
            self.card.append(&row);
            rows.push(row);
        }
        *self.rows.borrow_mut() = rows;
        self.mark();
    }

    fn mark(&self) {
        for (i, row) in self.rows.borrow().iter().enumerate() {
            if i == self.selected.get() {
                row.add_css_class("selected");
            } else {
                row.remove_css_class("selected");
            }
        }
    }

    fn command(self: &Rc<Self>, command: Cmd) {
        if self.active.get() {
            // Sem o teclado ainda, um toque novo (o anterior não foi confirmado); com ele, o Tab com o Alt
            // segurado. Antes, um toque a menos de ~200 ms do anterior descia na lista (visto na VM)
            let count = self.windows.borrow().len();
            let selected = if self.window.is_active() {
                sl::step(self.selected.get(), count, command)
            } else {
                sl::retap(self.selected.get(), count, command)
            };
            self.selected.set(selected);
            self.mark();
            return;
        }
        let windows = sway_tree().map(|tree| self.order.borrow().windows(&tree)).unwrap_or_default();
        if windows.len() < 2 {
            return;
        }
        self.refresh_theme();
        self.selected.set(sl::first_selection(windows.len(), command));
        *self.windows.borrow_mut() = windows;
        self.active.set(true);
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.fill();
        // Transparente até o prazo: um toque rápido troca sem o cartão piscar
        self.window.set_opacity(0.0);
        self.window.set_visible(true);
        self.window.present();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(delay("SWAY_SWITCHER_SHOW_MS", 150), move || {
            if let Some(me) = weak.upgrade() {
                if me.active.get() && me.generation.get() == generation {
                    me.window.set_opacity(1.0);
                }
            }
        });
        // Sem o teclado nesse prazo (a tela bloqueada, outro programa com o teclado exclusivo), fecha
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(delay("SWAY_SWITCHER_FOCUS_MS", 1000), move || {
            if let Some(me) = weak.upgrade() {
                if me.active.get() && me.generation.get() == generation && !me.window.is_active() {
                    me.close_card();
                }
            }
        });
    }

    fn close_card(&self) {
        self.active.set(false);
        self.window.set_visible(false);
    }

    fn commit(&self) {
        if !self.active.get() {
            return;
        }
        let target = self.windows.borrow().get(self.selected.get()).map(|w| w.id);
        self.close_card();
        if let Some(id) = target {
            focus(id);
        }
    }

    fn key_pressed(self: &Rc<Self>, key: gdk::Key) {
        if !self.active.get() {
            return;
        }
        match key {
            gdk::Key::Escape => self.close_card(),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => self.commit(),
            other => {
                if let Some(command) = key_move(other) {
                    self.command(command);
                }
            }
        }
    }
}

/// Os eventos "window" do Sway numa linha à parte; a principal só recebe o evento pronto. O fim da saída
/// (o Sway saiu) encerra: o laço do config abre de novo na próxima sessão.
fn watch_sway(switcher: &Rc<Switcher>, main_loop: &glib::MainLoop) -> Option<std::process::Child> {
    // setpriv: o swaymsg morre junto com o residente, mesmo num SIGKILL
    let mut child = swaydesk_ui::cmd::child_env(&mut Command::new("setpriv"))
        .args(["--pdeathsig", "TERM", "swaymsg", "-t", "subscribe", "-m", r#"["window"]"#])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| warn(&format!("swaymsg: {err}")))
        .ok()?;
    let stdout = child.stdout.take()?;
    let weak = Rc::downgrade(switcher);
    let main_loop = main_loop.clone();
    glib::MainContext::default().spawn_local(async move {
        let mut reader = Some((BufReader::new(stdout), sl::EventReader::default()));
        loop {
            let Some((mut lines, mut events)) = reader.take() else { break };
            let result = gio::spawn_blocking(move || {
                let mut found = Vec::new();
                let mut line = Vec::new();
                // Bytes: um título com UTF-8 inválido (comum no XWayland) não pode parar a leitura
                let read = lines.read_until(b'\n', &mut line);
                if let Ok(n) = read {
                    if n > 0 {
                        found.extend(events.feed(&String::from_utf8_lossy(&line)));
                    }
                }
                (lines, events, read, found)
            })
            .await;
            let Ok((lines, events, Ok(read), found)) = result else { break };
            if read == 0 {
                break;
            }
            if let Some(me) = weak.upgrade() {
                for event in &found {
                    me.on_event(event);
                }
            }
            reader = Some((lines, events));
        }
        main_loop.quit();
    });
    Some(child)
}

fn daemon() -> ExitCode {
    resident::set_process_name(PROGRAM);
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if Path::new(&argv0).file_name() != Some(PROGRAM.as_ref()) {
        reexec_as_program();
    }
    // O renderizador cairo: residente, a menor memória (antes dos sinais do GLib e de qualquer linha)
    swaydesk_ui::use_cairo_renderer();
    let Some(_lock) = resident::take_lock(&resident::lockfile(PROGRAM), Duration::ZERO) else { return ExitCode::SUCCESS };
    swaydesk_common::theme_resources::register_theme_resources();
    if let Err(err) = gtk4::init() {
        warn(&format!("could not start GTK: {err}"));
        return ExitCode::FAILURE;
    }
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }
    i18n::init(&config::load());
    let order = sway_tree().map(|tree| Order::from_tree(&tree)).unwrap_or_default();
    let switcher = Switcher::new(order);
    let main_loop = glib::MainLoop::new(None, false);
    // Os sinais antes do PID: um SIGUSR1 sem o tratamento encerraria o programa
    for (signal, command) in [(libc::SIGUSR1, Cmd::Next), (libc::SIGUSR2, Cmd::Prev)] {
        let switcher = switcher.clone();
        glib::unix_signal_add_local(signal, move || {
            switcher.command(command);
            glib::ControlFlow::Continue
        });
    }
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }
    let mut events = watch_sway(&switcher, &main_loop);
    let pidfile = resident::pidfile(PROGRAM);
    if let Err(err) = resident::write_pid(&pidfile) {
        warn(&format!("could not write {}: {err}", pidfile.display()));
    }
    main_loop.run();
    resident::remove_own_pid(&pidfile, PROGRAM);
    if let Some(child) = events.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [arg] if arg == "--check" => ExitCode::SUCCESS,
        [arg] if arg == "daemon" => daemon(),
        [arg] if parse_command(arg).is_some() => send(parse_command(arg).unwrap_or(Cmd::Next)),
        _ => {
            i18n::init(&config::load());
            eprintln!("{}", i18n::tr("usage: sway-switcher daemon|next|prev"));
            ExitCode::from(2)
        }
    }
}
