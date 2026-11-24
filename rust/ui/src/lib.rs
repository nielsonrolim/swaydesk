//! A base dos popups da Waybar em GTK 4: o porte de `home/.local/bin/waybar_popup.py`.
//!
//! Cada popup só traz o conteúdo: monta a janela com [`PopupWindow::new`] e chama [`run_popup`] no
//! `main`. Os contratos são os do Python, para as duas versões conviverem:
//! - o arquivo de PID `$XDG_RUNTIME_DIR/<nome>.pid` e a trava `<nome>.lock`;
//! - o processo roda com o nome do programa no argv[0] e no comm (o `pgrep -x` e a conferência do
//!   `/proc` acham do mesmo jeito);
//! - abrir de novo fecha (SIGTERM ao aberto), e abrir um popup fecha os outros da lista [`POPUPS`];
//! - o motivo de cada fechamento vai para `$XDG_RUNTIME_DIR/waybar-popups.log`.
use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::{config, i18n, resident};

pub mod cmd;
pub mod list;
pub mod modal;

/// Os popups da barra: abrir um fecha os outros (a mesma lista do `waybar_popup.py`).
pub const POPUPS: &[&str] = &[
    "waybar-weather", "waybar-calendar", "waybar-battery-popup", "waybar-audio", "waybar-network-popup",
    "waybar-bluetooth-popup", "waybar-brightness-popup", "waybar-caffeine-popup", "waybar-clipboard-popup",
    "waybar-notifications-popup", "waybar-system-popup", "waybar-media-popup",
];
/// Quanto o popup espera, depois de perder o foco, pelo foco de volta antes de fechar.
const FOCUS_OUT_GRACE: Duration = Duration::from_millis(200);
/// Logo depois de aparecer ou mudar de tamanho, o Sway tira o foco da superfície por um instante.
const RENDER_GRACE: Duration = Duration::from_millis(700);

/// O CSS comum: o cartão, os títulos e os links. Tudo ASCII, como no Python.
pub const BASE_CSS: &str = r#"
window { background: transparent; }
/* Quase invisível, mas não zero: totalmente transparente, o GTK 4 não desenha quadro nenhum, a superfície
   fica sem buffer, o Sway não a mapeia e o clique fora não chega (visto no WAYLAND_DEBUG da VM) */
window.catcher { background: rgba(0, 0, 0, 0.01); }
.card {
  background: rgba(46, 46, 51, 0.97);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 14px 16px;
  color: #dadada;
}
.section { color: #8a8a8f; font-size: 12px; font-weight: bold; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
/* O Adwaita do GTK 4 sublinha o texto dos links; o do GTK 3, não */
button.link > label { text-decoration-line: none; }
"#;

/// O motivo de cada fechamento, para investigar os inesperados.
pub fn log(program: &str, message: &str) {
    let path = resident::runtime_dir().join("waybar-popups.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = glib::DateTime::now_local().ok().and_then(|t| t.format("%F %T").ok()).unwrap_or_default();
        let _ = writeln!(file, "{now} {program}: {message}");
    }
}

/// O campo de busca de todos os componentes, como o do rofi: a classe do config::SEARCH_CSS (que vem no
/// theme_css e no config::accent_css) e o mesmo texto de exemplo. `dark`: o fundo escuro do rofi, nos popups
/// escuros; sem ele, o fundo do tema GTK (os apps em janela, claros ou escuros).
pub fn search_entry(dark: bool) -> gtk4::SearchEntry {
    let entry = gtk4::SearchEntry::new();
    entry.add_css_class(config::SEARCH_CLASS);
    if dark {
        entry.add_css_class(config::SEARCH_DARK_CLASS);
    }
    entry.set_placeholder_text(Some(&i18n::tr("Search…")));
    entry
}

pub fn label(text: &str, classes: &[&str], xalign: f32) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(xalign);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

fn system_font_family() -> Option<String> {
    let name = gtk4::Settings::default()?.gtk_font_name()?;
    pango::FontDescription::from_string(&name).family().map(|f| f.to_string())
}

/// As cores do destaque da config e as fontes, em CSS: o THEME_CSS do `waybar_popup.py`. Vem antes do CSS
/// de cada janela. Depois do gtk4::init (lê a fonte do sistema nas configurações do GTK).
pub fn theme_css() -> String {
    let palette = config::palette(&config::load());
    format!("{}{}", config::accent_css(&palette), config::font_css(system_font_family().as_deref()))
}

/// A janela sob a barra que fecha com Esc, clique fora, foco em outra janela ou troca de workspace.
pub struct PopupWindow {
    pub program: &'static str,
    pub window: gtk4::Window,
    catchers: RefCell<Vec<gtk4::Window>>,
    main_loop: glib::MainLoop,
    had_focus: Cell<bool>,
    /// Quando mudou de tamanho pela última vez: o focus-out logo depois é do Sway, e não fecha
    pub rendered_at: Cell<Instant>,
    focus_timer: RefCell<Option<glib::SourceId>>,
    closing: Cell<bool>,
    /// O swaymsg que assina os eventos: ao fechar sem evento do Sway, ele é encerrado, e a linha que o
    /// lê termina (sem isso, num processo que segue vivo, como a dock, cada janela deixaria uma presa)
    watcher_pid: Arc<AtomicI32>,
    /// O aviso de monitores do display, desconectado com a janela
    monitors_handler: RefCell<Option<(gio::ListModel, glib::SignalHandlerId)>>,
    /// Consultada antes de fechar: false adia (o popup de Bluetooth enquanto pareia)
    close_guard: RefCell<Option<Rc<dyn Fn(&str) -> bool>>>,
    /// As camadas do clique fora foram tiradas de propósito: um monitor novo não as traz de volta
    catchers_dropped: Cell<bool>,
}

thread_local! {
    /// O que o SIGTERM (um segundo clique no ícone, ou outro popup abrindo) faz; sem nada, sai já
    static TERMINATE: RefCell<Option<Box<dyn Fn() -> bool>>> = const { RefCell::new(None) };
}

/// Troca o que o SIGTERM faz: o popup de Bluetooth passa pelo próprio `close`, que pode adiar. `f`
/// devolve se cuidou do sinal (fechou ou adiou); false sai já.
pub fn on_terminate(f: impl Fn() -> bool + 'static) {
    TERMINATE.with(|t| *t.borrow_mut() = Some(Box::new(f)));
}

impl PopupWindow {
    /// `right`: junto à direita, sob os ícones; sem ele, centralizado. `css`: o do popup, depois do
    /// comum e das cores da config.
    pub fn new(program: &'static str, namespace: &str, css: &str, right: bool, main_loop: &glib::MainLoop) -> Rc<PopupWindow> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&format!("{}{BASE_CSS}{css}", theme_css()));
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(true);
            settings.set_gtk_cursor_blink(false); // a piscada redesenha a janela inteira (lição do drawer)
        }

        let window = gtk4::Window::new();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay); // acima da camada do clique fora
        window.set_anchor(Edge::Top, true);
        window.set_margin(Edge::Top, 4); // abaixo da barra
        if right {
            window.set_anchor(Edge::Right, true);
            window.set_margin(Edge::Right, 6);
        }
        // Exclusivo: com o ON_DEMAND, o Sway devolve o teclado à janela do workspace logo depois de o
        // popup abrir. Os atalhos do Sway continuam valendo, e o clique fora fecha pelos catchers
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_namespace(namespace);

        let popup = Rc::new(PopupWindow {
            program,
            window,
            catchers: RefCell::new(Vec::new()),
            main_loop: main_loop.clone(),
            had_focus: Cell::new(false),
            rendered_at: Cell::new(Instant::now()),
            focus_timer: RefCell::new(None),
            closing: Cell::new(false),
            watcher_pid: Arc::new(AtomicI32::new(0)),
            monitors_handler: RefCell::new(None),
            close_guard: RefCell::new(None),
            catchers_dropped: Cell::new(false),
        });
        *popup.catchers.borrow_mut() = click_catchers(&popup);
        if let Some(display) = gdk::Display::default() {
            let weak = Rc::downgrade(&popup);
            let monitors = display.monitors();
            let id = monitors.connect_items_changed(move |_, _, _, _| {
                if let Some(p) = weak.upgrade() {
                    p.rebuild_catchers();
                }
            });
            *popup.monitors_handler.borrow_mut() = Some((monitors, id));
        }
        popup.connect();
        popup.watch_sway();
        popup
    }

    fn connect(self: &Rc<Self>) {
        let keys = gtk4::EventControllerKey::new();
        let p = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(p) = p.upgrade() {
                    p.close("Esc");
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);

        // Fecha ao perder o foco só se chegou a tê-lo, e só se ele não voltar logo: o Sway tira e
        // devolve o foco quando o ponteiro passa do catcher para o popup
        let p = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            let Some(p) = p.upgrade() else { return };
            if window.is_active() {
                p.had_focus.set(true);
                if let Some(timer) = p.focus_timer.borrow_mut().take() {
                    timer.remove();
                }
            } else if p.had_focus.get() && p.focus_timer.borrow().is_none() {
                // Dentro da carência do render, a conferência fica para o fim dela, e não é descartada:
                // o foco pode ter saído de verdade, sem evento de janela nem de workspace
                let wait = FOCUS_OUT_GRACE.max(RENDER_GRACE.saturating_sub(p.rendered_at.get().elapsed()));
                let weak = Rc::downgrade(&p);
                let timer = glib::timeout_add_local_once(wait, move || {
                    if let Some(p) = weak.upgrade() {
                        p.focus_timer.borrow_mut().take();
                        if !p.window.is_active() {
                            p.close("focus-out");
                        }
                    }
                });
                *p.focus_timer.borrow_mut() = Some(timer);
            }
        });
    }

    /// Mostra a janela (e marca o momento: o focus-out do Sway logo depois não fecha).
    pub fn show(&self) {
        self.rendered_at.set(Instant::now());
        self.window.set_visible(true);
        self.window.present();
    }

    /// Fecha, a não ser que a guarda adie (ela guarda o motivo e fecha depois, pelo próprio `close`).
    pub fn close(&self, reason: &str) {
        if self.closing.get() {
            return;
        }
        // Clonada antes de chamar: a guarda pode trocar a si mesma sem pânico do RefCell
        let guard = self.close_guard.borrow().clone();
        if let Some(guard) = guard {
            if !guard(reason) {
                return;
            }
        }
        self.closing.set(true);
        log(self.program, reason);
        self.stop_watcher();
        self.main_loop.quit();
    }

    /// A guarda consultada antes de cada fechamento (`false` adia).
    pub fn set_close_guard(&self, guard: impl Fn(&str) -> bool + 'static) {
        self.close_guard.replace(Some(Rc::new(guard)));
    }

    /// Tira as camadas do clique fora: o popup de Bluetooth faz isso ao parear, porque elas bloqueariam o
    /// diálogo de confirmação do agente.
    pub fn drop_catchers(&self) {
        self.catchers_dropped.set(true);
        for catcher in self.catchers.borrow_mut().drain(..) {
            catcher.destroy();
        }
    }

    /// Põe de volta as camadas do clique fora tiradas pelo `drop_catchers`.
    pub fn restore_catchers(self: &Rc<Self>) {
        if self.catchers_dropped.replace(false) {
            self.rebuild_catchers();
        }
    }

    /// Encerra o swaymsg dos eventos, se ainda estiver lá: a linha que o lê recebe o fim e termina.
    fn stop_watcher(&self) {
        // -1 é "cancelado": se a linha ainda não abriu o swaymsg, ela vê isso e nem abre
        let pid = self.watcher_pid.swap(WATCHER_CANCELLED, Ordering::SeqCst);
        let _ = LIVE_WATCHER.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: kill só recebe números; o processo é filho desta linha e só sai com o wait dela
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    /// Fecha quando outra janela recebe o foco ou o workspace muda. Os eventos da janela focada ao
    /// abrir são ignorados: clicar no popup tira o foco dela, e o Sway a anuncia de novo.
    fn watch_sway(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let pid = self.watcher_pid.clone();
        glib::MainContext::default().spawn_local(async move {
            // Numa linha bloqueante do GIO, e sem polling: a principal só acorda com o evento
            let reason = gio::spawn_blocking(move || watch_sway_events(&pid)).await.ok().flatten();
            match (reason, weak.upgrade()) {
                (Some(reason), Some(p)) => p.close(&reason),
                (None, Some(p)) if !p.closing.get() => {
                    eprintln!("{}: the Sway events stopped: the popup no longer closes by focus or workspace", p.program);
                }
                _ => {}
            }
        });
    }
}

/// Espera, numa linha própria, o evento que fecha o popup. None: o swaymsg não abriu ou terminou sem ele
/// (o IPC do Sway caiu).
/// O valor de `watcher_pid` depois de a janela fechar.
const WATCHER_CANCELLED: i32 = -1;

/// O swaymsg dos eventos em uso no processo, para o fim do [`run_single`]. O setpriv --pdeathsig não basta:
/// ele nasce numa linha do GIO, e na saída do processo o Linux o passa para outra linha do mesmo processo sem
/// mandar o sinal. Um popup fechado por SIGTERM (abrir de novo, ou abrir outro popup) saía do laço sem passar
/// pelo close, e o swaymsg ficava órfão (visto numa VM no #117, reproduzido no Sway headless).
static LIVE_WATCHER: AtomicI32 = AtomicI32::new(0);

/// Encerra o swaymsg dos eventos que ainda estiver vivo. Chamado ao sair do laço principal.
fn kill_live_watcher() {
    let pid = LIVE_WATCHER.swap(0, Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill só recebe números; o processo é filho deste e ainda não foi esperado
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
}

fn watch_sway_events(pid: &AtomicI32) -> Option<String> {
    if pid.load(Ordering::SeqCst) == WATCHER_CANCELLED {
        return None; // a janela fechou antes de a linha começar
    }
    // Com prazo: um IPC travado não prende a linha antes da assinatura
    let start_id = Command::new("timeout")
        .args(["2", "swaymsg", "-t", "get_tree"])
        .output()
        .ok()
        .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out.stdout).ok())
        .and_then(|tree| focused_window(&tree));
    // setpriv: o swaymsg morre junto com o popup, mesmo num SIGKILL
    // O child_env desbloqueia os sinais: esta linha do GIO pode ter nascido com o TERM bloqueado
    let mut child = cmd::child_env(&mut Command::new("setpriv"))
        .args(["--pdeathsig", "TERM", "swaymsg", "-t", "subscribe", "-m", r#"["window","workspace"]"#])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let id = child.id() as i32;
    // O registro do processo vem antes de publicar no pid: um stop_watcher no meio acha os dois e não deixa
    // o LIVE_WATCHER apontar para um filho já colhido. Antes de cada wait, ele sai do registro: um PID colhido
    // pode ser reusado por outro processo do usuário
    LIVE_WATCHER.store(id, Ordering::SeqCst);
    // A janela pode ter fechado durante o get_tree (até 2 s): aí ninguém mais encerraria este swaymsg
    if pid.compare_exchange(0, id, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        let _ = LIVE_WATCHER.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let stdout = child.stdout.take()?;
    let mut reason = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let change = event.get("change").and_then(|c| c.as_str()).unwrap_or_default().to_string();
        if let Some(container) = event.get("container") {
            let id = container.get("id").and_then(|i| i.as_i64());
            if (change == "focus" || change == "new") && id != start_id {
                reason = Some(format!("sway window {change} id={id:?} (início {start_id:?})"));
                break;
            }
        } else if change == "focus" || change == "init" {
            reason = Some(format!("sway workspace {change}"));
            break;
        }
    }
    // Antes do wait (depois dele o PID pode ser de outro processo), sem apagar um "cancelado"
    let _ = pid.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
    let _ = child.kill();
    let _ = LIVE_WATCHER.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
    let _ = child.wait(); // sem zumbi
    reason
}

impl PopupWindow {
    /// Refaz os catchers, um por monitor: quando um monitor liga ou desliga com a janela aberta.
    fn rebuild_catchers(self: &Rc<Self>) {
        for catcher in self.catchers.borrow_mut().drain(..) {
            catcher.destroy();
        }
        if self.catchers_dropped.get() {
            return;
        }
        let catchers = click_catchers(self);
        *self.catchers.borrow_mut() = catchers;
    }
}

impl Drop for PopupWindow {
    fn drop(&mut self) {
        self.stop_watcher();
        if let Some((monitors, id)) = self.monitors_handler.borrow_mut().take() {
            monitors.disconnect(id);
        }
        for catcher in self.catchers.borrow().iter() {
            catcher.destroy();
        }
    }
}

/// O id da janela com o foco do Sway ou, se o foco está numa camada (o próprio popup, que já pode tê-lo
/// ao abrir), o da última focada: a primeira da lista "focus" em cada nível. None: workspace vazio.
pub fn focused_window(tree: &serde_json::Value) -> Option<i64> {
    let mut node = tree;
    loop {
        if node.get("focused").and_then(|f| f.as_bool()) == Some(true) {
            return node.get("id").and_then(|i| i.as_i64());
        }
        let children: Vec<&serde_json::Value> = ["nodes", "floating_nodes"]
            .iter()
            .filter_map(|k| node.get(*k).and_then(|v| v.as_array()))
            .flatten()
            .collect();
        if children.is_empty() {
            let kind = node.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            return if kind == "con" || kind == "floating_con" { node.get("id").and_then(|i| i.as_i64()) } else { None };
        }
        let first = node.get("focus").and_then(|f| f.as_array()).and_then(|f| f.first()).and_then(|f| f.as_i64());
        node = children.into_iter().find(|c| first.is_some() && c.get("id").and_then(|i| i.as_i64()) == first)?;
    }
}

/// Camada transparente sob o popup, uma por monitor: um clique fora dele fecha, mesmo na janela que já
/// tinha o foco. A zona exclusiva da barra fica de fora: os ícones continuam clicáveis.
fn click_catchers(popup: &Rc<PopupWindow>) -> Vec<gtk4::Window> {
    let weak = Rc::downgrade(popup);
    // A barra fica de fora: um clique nela (outro ícone) vai à Waybar, que troca de popup
    catcher_windows("popup-click-catcher", false, Rc::new(move || {
        if let Some(p) = weak.upgrade() {
            p.close("clique fora");
        }
    }))
}

/// As camadas que pegam o clique fora: uma por monitor, na camada de cima (acima das janelas, abaixo do overlay
/// e das janelas override-redirect do Xwayland), presa às quatro bordas, sem teclado, quase transparente (a
/// classe `catcher` do BASE_CSS). Um clique em qualquer uma chama `on_click`. O namespace "popup-click-catcher"
/// fica fora do blur do SwayFX (desktop::BLUR_LAYERS): borraria a tela toda. Com `cover_bars`, a camada cobre
/// também a área que a Waybar reserva (exclusive zone -1): um clique na barra a fecha; sem, ela termina na borda
/// da barra, e o clique vai à Waybar.
pub fn catcher_windows(namespace: &str, cover_bars: bool, on_click: Rc<dyn Fn()>) -> Vec<gtk4::Window> {
    let mut windows = Vec::new();
    let Some(display) = gdk::Display::default() else { return windows };
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let catcher = gtk4::Window::new();
        catcher.add_css_class("catcher");
        catcher.init_layer_shell();
        catcher.set_layer(Layer::Top);
        catcher.set_monitor(&monitor); // o crate 0.4 do Debian 13 recebe a referência direto
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            catcher.set_anchor(edge, true);
        }
        catcher.set_keyboard_mode(KeyboardMode::None);
        if cover_bars {
            catcher.set_exclusive_zone(-1);
        }
        catcher.set_namespace(namespace);
        // Um filho que a preenche: a área do clique é a janela inteira
        let area = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        area.set_hexpand(true);
        area.set_vexpand(true);
        catcher.set_child(Some(&area));
        let click = gtk4::GestureClick::new();
        click.set_button(0);
        let on_click = on_click.clone();
        click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            on_click();
        });
        catcher.add_controller(click);
        catcher.set_visible(true);
        if std::env::var_os("SWAYDESK_UI_DEBUG").is_some() {
            let c = catcher.clone();
            glib::timeout_add_local_once(Duration::from_secs(1), move || {
                eprintln!("catcher: mapped={} {}x{}", c.is_mapped(), c.width(), c.height());
            });
        }
        windows.push(catcher);
    }
    if std::env::var_os("SWAYDESK_UI_DEBUG").is_some() {
        eprintln!("catchers: {} for {} monitors", windows.len(), monitors.n_items());
    }
    windows
}

/// Bloqueia ou desbloqueia o SIGTERM e o SIGINT nesta linha (a principal, antes de criar outras).
fn set_blocked(blocked: bool) {
    // SAFETY: só manipula um sigset_t local e a máscara de sinais da linha atual
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(if blocked { libc::SIG_BLOCK } else { libc::SIG_UNBLOCK }, &set, std::ptr::null_mut());
    }
}

/// O renderizador do GTK 4 (cairo, a não ser que o ambiente já escolha outro): o do
/// [`swaydesk_common::renderer`], o mesmo da grade e do bloqueio. Público para as janelas que não passam pelo
/// `run_popup` (a dock): chame antes de abrir linhas e do gtk4::init.
pub use swaydesk_common::renderer::use_cairo_renderer;

/// Abre o popup, ou fecha o que já estiver aberto (abrir de novo fecha), e fecha os outros popups da
/// barra. `build` monta a janela e devolve o que a mantém viva; `notify`, chamado ao abrir e ao fechar
/// (o ícone esconde a dica com o popup aberto).
pub fn run_popup<T>(program: &'static str, build: impl FnOnce(&glib::MainLoop) -> Option<T>, notify: impl Fn()) -> ExitCode {
    let others: Vec<&str> = POPUPS.iter().copied().filter(|p| *p != program).collect();
    run_single(program, &others, |_| true, build, notify)
}

/// Os argumentos (depois do nome) do programa do arquivo de PID, se ele ainda estiver lá: o
/// `live_process` do `waybar_common.py`. O nome vem no argv[0], ou no argv[1] depois do interpretador.
fn live_args(pidfile: &Path, name: &str) -> Option<Vec<String>> {
    let pid = resident::live_pid(pidfile, name)?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    let at = args.iter().take(2).position(|a| Path::new(std::ffi::OsStr::from_bytes(a)).file_name() == Some(name.as_ref()))?;
    Some(args[at + 1..].iter().filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
}

/// O `run_single` do `waybar_popup.py`: abre a janela, ou fecha a que já estiver aberta. `others`: os
/// programas que fecham quando este abre. `closes_open` recebe os argumentos do aberto e diz se só fecha
/// (true) ou se este abre no lugar dele (o `sway-capture` de outro tipo). O [`run_popup`] é este com
/// os popups da barra e sempre só fechando.
pub fn run_single<T>(
    program: &'static str,
    others: &[&str],
    closes_open: impl Fn(&[String]) -> bool,
    build: impl FnOnce(&glib::MainLoop) -> Option<T>,
    notify: impl Fn(),
) -> ExitCode {
    resident::set_process_name(program);
    resident::run_as(program); // o /proc mostra o nome do programa a quem confere o arquivo de PID
    // Um 2º clique (ou outro popup abrindo) manda SIGTERM durante a subida, antes do handler do GLib: com a
    // ação padrão o processo morreria sem apagar o pidfile. Bloqueados até lá, o sinal fica pendente e
    // é entregue ao GLib, que fecha limpo. Depois do reexec: a máscara passa pelo exec
    set_blocked(true);
    use_cairo_renderer();
    let pidfile = resident::pidfile(program);
    {
        // A trava evita duas janelas quando dois pedidos chegam juntos
        let Some(_lock) = resident::take_lock(&resident::lockfile(program), Duration::from_secs(2)) else {
            return ExitCode::SUCCESS;
        };
        // Os argumentos antes do sinal: depois dele, o aberto pode já ter saído
        let open_args = live_args(&pidfile, program);
        if resident::signal_pid(&pidfile, program, libc::SIGTERM) {
            // Quem fecha diz por quê: o aberto só registra o sinal
            log(program, "opened again: SIGTERM to the open one");
            if closes_open(&open_args.unwrap_or_default()) {
                return ExitCode::SUCCESS; // estava aberto: só fecha
            }
        }
        for other in others {
            if resident::signal_pid(&resident::pidfile(other), other, libc::SIGTERM) {
                log(other, &format!("SIGTERM: {program} opened"));
            }
        }
        if let Err(err) = resident::write_pid(&pidfile) {
            eprintln!("{program}: could not write {}: {err}", pidfile.display());
        }
    }
    let code = (|| {
        swaydesk_common::theme_resources::register_theme_resources();
        if let Err(err) = gtk4::init() {
            eprintln!("{program}: could not start GTK: {err}");
            return ExitCode::FAILURE;
        }
        i18n::init(&config::load());
        let main_loop = glib::MainLoop::new(None, false);
        for (signal, name) in [(libc::SIGTERM, "SIGTERM"), (libc::SIGINT, "SIGINT")] {
            let main_loop = main_loop.clone();
            // Clicaram de novo no ícone ou abriram outro popup. O sinal não passa pelo close: sem esta linha,
            // o fechamento não deixaria motivo no log
            glib::unix_signal_add_local(signal, move || {
                log(program, name);
                let handled = TERMINATE.with(|t| t.borrow().as_ref().is_some_and(|f| f()));
                if !handled {
                    main_loop.quit();
                }
                glib::ControlFlow::Continue
            });
        }
        set_blocked(false); // um pendente chega agora ao GLib
        let Some(_keep) = build(&main_loop) else { return ExitCode::FAILURE };
        notify();
        main_loop.run();
        kill_live_watcher(); // sair pelo SIGTERM não passa pelo close do popup
        ExitCode::SUCCESS
    })();
    resident::remove_own_pid(&pidfile, program);
    notify();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_window_follows_the_focus_list() {
        let tree = serde_json::json!({
            "type": "root", "focus": [2], "nodes": [
                {"id": 1, "type": "output", "focus": [], "nodes": []},
                {"id": 2, "type": "output", "focus": [4], "nodes": [
                    {"id": 3, "type": "con", "nodes": []},
                    {"id": 4, "type": "workspace", "focus": [5], "nodes": [], "floating_nodes": [
                        {"id": 5, "type": "floating_con", "nodes": []}]}]}]});
        assert_eq!(focused_window(&tree), Some(5));
        assert_eq!(focused_window(&serde_json::json!({"type": "workspace", "nodes": []})), None);
    }

    #[test]
    fn the_popup_list_matches_python() {
        let Some(path) = swaydesk_common::reference::repo_file(env!("CARGO_MANIFEST_DIR"), "home/.local/bin/waybar_popup.py") else { return };
        let python = std::fs::read_to_string(path).unwrap();
        for name in POPUPS {
            assert!(python.contains(&format!("\"{name}\"")), "{name}");
        }
    }
}
