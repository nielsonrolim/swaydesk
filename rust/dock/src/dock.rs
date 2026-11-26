//! A dock em si: uma janela por monitor, os itens, os menus, o arrasto dos fixados e a animação.
//!
//! A lista de janelas vem do Sway: cada evento agenda uma leitura da árvore numa linha do GIO, e nada de
//! GTK fora da linha principal. Sem foco de teclado: a dock nunca tira o foco da janela em uso. Os
//! itens tratam o mouse direto (um GestureDrag), para distinguir clique de arrasto.
//!
//! Cada closure de widget guarda só referências fracas (Weak) à dock e aos itens: uma dock refeita (um
//! monitor que entrou ou um reload) solta a anterior por inteiro.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, graphene, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::{config, i18n::tr, resident};
use swaydesk_dock_logic as dl;
use swaydesk_ui::cmd;

use crate::drag_view::DragView;
use crate::{requests_path, warn};

const REFRESH: Duration = Duration::from_millis(100);
const POLL: Duration = Duration::from_secs(1);
/// Só com um menu aberto ou um arrasto: confere se o fim deles se perdeu.
const STUCK_CHECK: Duration = Duration::from_secs(2);
/// Com o menu pedindo o teclado: quanto esperar por ele antes de abrir o menu mesmo assim
const KEYBOARD_WAIT_MS: u64 = 250;
const FRAME: Duration = Duration::from_millis(16);
const GLYPH_SCALE: f64 = 0.75;
const HOTSPOT_HEIGHT: i32 = 2;
const MENU_TITLE_CHARS: i32 = 50;
/// O tempo do ponteiro parado sobre um item até a dica aparecer (o mesmo do GTK).
const TIP_DELAY: Duration = Duration::from_millis(500);
/// A margem lateral de cada item no CSS (4 px de cada lado): a largura de um lugar na dock.
const ITEM_MARGIN: f64 = 8.0;
const ICON_APPS: &str = "\u{f003b}"; // apps (Material da Nerd Font)
const ICON_APP: &str = "\u{f08c6}"; // application-outline
const NEW_WINDOW_ACTIONS: &[&str] = &["new-window", "new_window", "NewWindow", "New", "Window"];

// Fundo e cantos no #dock, não na janela: a margem de 6 px é parte da janela e, com fundo nela,
// apareceria uma barra embaixo da dock
const CSS: &str = r#"
window { background: transparent; }
/* Quase invisível, mas não zero: sem nada desenhado, o GTK 4 não manda buffer, o Sway não mapeia a
   faixa e o ponteiro na borda não chega (a mesma lição dos catchers dos popups) */
window.hotspot { background: rgba(0, 0, 0, 0.01); }
#dock {
  background: rgba(46, 46, 51, 0.90);
  border-radius: 12px;
  margin-bottom: 6px;
  padding: 6px 10px;
}
.dock-item {
  color: #dadada; border-radius: 8px;
  padding: 4px 4px 2px 4px; margin: 0 4px;
}
.dock-item:hover { background-color: #4c4c50; }
/* Arrastado, com a animação ligada: levantado (a escala e a opacidade vêm do DragView) */
.dock-item.dragging { background-color: #4c4c50; box-shadow: 0 3px 8px rgba(0, 0, 0, 0.45); }
.glyph { font-family: "Symbols Nerd Font"; }
.dots { min-height: 4px; }
.dot { min-width: 4px; min-height: 4px; border-radius: 2px; background: #8a8a8f; }
.focused .dot { background: @accent; }
.urgent .dot { background: #f27835; }
popover.menu > contents {
  background: #2e2e33; color: #dadada; border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px; padding: 4px; box-shadow: none;
}
popover.menu button {
  background: none; border: none; box-shadow: none; color: #dadada;
  padding: 5px 10px; border-radius: 6px; font-size: 13px; min-height: 0;
}
popover.menu button:hover { background: #4c4c50; color: #ffffff; }
popover.menu button > label { font-weight: normal; }
popover.menu separator { background: rgba(255, 255, 255, 0.06); min-height: 1px; margin: 4px 0; }
popover.tip > contents {
  background: rgba(0, 0, 0, 0.80); color: #ffffff; border-radius: 6px; padding: 4px 8px;
  box-shadow: none; font-size: 12px;
}
"#;

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| config::home().join(".config"));
    base.join("sway-dock").join("config.toml")
}

fn now_ms() -> i64 {
    glib::monotonic_time() / 1000
}

/// Com a grade de apps aberta, um clique na dock a esconde antes: o app abre sem ela por cima, e os menus
/// da dock, na camada de baixo, ficam à vista. Só se a grade disse que está à vista: uma grade antiga, sem
/// o pedido do SIGUSR2, morreria com o sinal.
fn hide_drawer() {
    if dl::drawer_shown_path(&resident::runtime_dir()).exists() {
        resident::signal_pid(&resident::pidfile("sway-drawer"), "sway-drawer", libc::SIGUSR2);
    }
}

fn monitor_rect(monitor: &gdk::Monitor) -> dl::Rect {
    let g = monitor.geometry();
    (g.x() as i64, g.y() as i64, g.width() as i64, g.height() as i64)
}

fn all_monitors() -> Vec<gdk::Monitor> {
    let Some(display) = gdk::Display::default() else { return vec![] };
    let monitors = display.monitors();
    (0..monitors.n_items()).filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>()).collect()
}

fn system_font_family() -> Option<String> {
    let name = gtk4::Settings::default()?.gtk_font_name()?;
    pango::FontDescription::from_string(&name).family().map(|f| f.to_string())
}

/// Um comando interno e curto do Sway: sem sessão própria, e uma linha espera o fim (sem zumbi).
fn sway(command: &str) {
    match Command::new("swaymsg").args(["-q", command]).stdin(Stdio::null()).stdout(Stdio::null()).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => warn(&format!("could not run swaymsg: {err}")),
    }
}

fn expand_home(text: &str) -> String {
    match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", config::home().display()),
        _ => text.to_string(),
    }
}

fn desktop_info(key: &str) -> Option<gio::DesktopAppInfo> {
    gio::DesktopAppInfo::new(&format!("{key}.desktop"))
}

/// Sem o cairo que a dock escolheu para si: os apps em GTK 4 desenhariam pela CPU.
fn launch_context() -> Option<gdk::AppLaunchContext> {
    let context = gdk::Display::default().map(|d| d.app_launch_context())?;
    cmd::clean_launch_context(&context);
    swaydesk_common::app_scope::scope_launch_context(&context); // num serviço do usuário, cada app no seu escopo
    Some(context)
}

/// Abre o app, ou uma janela nova se o .desktop oferecer essa ação. O GLib solta o app do processo da
/// dock (ele não fica como filho dela); num serviço do usuário, o app abre já no seu escopo (app_scope::launch).
fn launch(key: &str) {
    let Some(info) = desktop_info(key) else { return };
    let context = launch_context();
    let context = context.as_ref().map(|c| c.upcast_ref::<gio::AppLaunchContext>());
    let actions = info.list_actions();
    let action = NEW_WINDOW_ACTIONS.iter().find(|a| actions.iter().any(|x| x == **a)).copied();
    if let Err(err) = swaydesk_common::app_scope::launch(&info, action, &[], context) {
        warn(&format!("could not open {key}: {}", err.message()));
    }
}

// --- Ícones ---

fn make_glyph(text: &str, size: i32) -> gtk4::Widget {
    // O tamanho da fonte vem do CSS da dock, que sabe o icon_size da config
    let glyph = gtk4::Label::new(Some(text));
    glyph.add_css_class("glyph");
    glyph.set_size_request(size, size);
    glyph.upcast()
}

/// O ícone do .desktop; sem ele, o nome no tema; sem nada disso, um glifo.
fn make_icon(info: Option<&gio::DesktopAppInfo>, key: &str, size: i32) -> gtk4::Widget {
    let theme = gdk::Display::default().map(|d| gtk4::IconTheme::for_display(&d));
    let mut image: Option<gtk4::Image> = None;
    if let Some(icon) = info.and_then(|i| i.icon()) {
        if let Some(file_icon) = icon.downcast_ref::<gio::FileIcon>() {
            image = file_icon.file().path().and_then(|path| {
                dl::icon_paths(&path.to_string_lossy())
                    .into_iter()
                    .filter(|candidate| Path::new(candidate).is_file())
                    .find_map(|candidate| gdk::Texture::from_filename(&candidate).ok())
                    .map(|texture| gtk4::Image::from_paintable(Some(&texture)))
            });
        } else if theme.as_ref().is_some_and(|t| t.has_gicon(&icon)) {
            image = Some(gtk4::Image::from_gicon(&icon));
        }
    }
    if image.is_none() {
        if let Some(theme) = &theme {
            image = [key.to_string(), key.to_lowercase()]
                .into_iter()
                .find(|name| theme.has_icon(name))
                .map(|name| gtk4::Image::from_icon_name(&name));
        }
    }
    match image {
        Some(image) => {
            image.set_pixel_size(size);
            image.set_size_request(size, size);
            image.upcast()
        }
        None => make_glyph(ICON_APP, size),
    }
}

// --- Apps instalados ---

/// Os apps instalados: de um app_id ou de um fixado para o .desktop.
struct Apps {
    entries: RefCell<Vec<dl::AppEntry>>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl Apps {
    fn new() -> Apps {
        let apps = Apps { entries: RefCell::new(vec![]), cache: RefCell::new(HashMap::new()) };
        apps.reindex();
        apps
    }

    fn reindex(&self) {
        let entries = gio::AppInfo::all()
            .into_iter()
            .filter_map(|info| info.downcast::<gio::DesktopAppInfo>().ok())
            .filter_map(|info| {
                let id = info.id()?.to_string();
                Some(dl::AppEntry {
                    id: id.strip_suffix(".desktop").unwrap_or(&id).to_string(),
                    wm_class: info.startup_wm_class().map(|c| c.to_string()).unwrap_or_default(),
                })
            })
            .collect();
        *self.entries.borrow_mut() = entries;
        self.cache.borrow_mut().clear();
    }

    fn resolve(&self, app_id: &str) -> Option<String> {
        if let Some(found) = self.cache.borrow().get(app_id) {
            return found.clone();
        }
        let found = dl::match_app(app_id, &self.entries.borrow());
        self.cache.borrow_mut().insert(app_id.to_string(), found.clone());
        found
    }
}

// --- A dica ---

/// A dica sobre um item. A do GTK 4 abre embaixo do widget e, na dock colada na borda da tela, saía
/// cortada (visto na VM): aqui é um Popover sem autohide (sem grab), acima do item.
struct Tip {
    popover: gtk4::Popover,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Tip {
    fn new(bar: &Rc<Bar>, widget: &gtk4::Box, text: &str) -> Rc<Tip> {
        let popover = gtk4::Popover::new();
        popover.add_css_class("tip");
        popover.set_has_arrow(false);
        popover.set_autohide(false);
        popover.set_position(gtk4::PositionType::Top);
        popover.set_can_target(false);
        popover.set_child(Some(&gtk4::Label::new(Some(text))));
        popover.set_parent(widget);
        let tip = Rc::new(Tip { popover, timer: RefCell::new(None) });
        let motion = gtk4::EventControllerMotion::new();
        let (b, t) = (Rc::downgrade(bar), Rc::downgrade(&tip));
        motion.connect_enter(move |_, _, _| {
            if let Some(tip) = t.upgrade() {
                tip.schedule(&b);
            }
        });
        let t = Rc::downgrade(&tip);
        motion.connect_leave(move |_| {
            if let Some(tip) = t.upgrade() {
                tip.hide();
            }
        });
        widget.add_controller(motion);
        tip
    }

    fn schedule(self: &Rc<Self>, bar: &Weak<Bar>) {
        self.hide();
        let (b, t) = (bar.clone(), Rc::downgrade(self));
        let timer = glib::timeout_add_local_once(TIP_DELAY, move || {
            let (Some(bar), Some(tip)) = (b.upgrade(), t.upgrade()) else { return };
            tip.timer.borrow_mut().take();
            // Nada de dica por cima de um menu, durante um arrasto ou com a dock saindo
            if bar.menu.borrow().is_none() && !bar.dragging.get() && bar.window.is_visible() && bar.slide.borrow().target == 0.0 {
                tip.popover.popup();
            }
        });
        *self.timer.borrow_mut() = Some(timer);
    }

    fn hide(&self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
        if self.popover.is_visible() {
            self.popover.popdown();
        }
    }

    fn remove(&self) {
        self.hide();
        self.popover.unparent();
    }
}

// --- Um item ---

/// Um app na dock. O mouse é tratado direto: soltar sem arrastar é clique.
struct DockItem {
    widget: gtk4::Box,
    dots: gtk4::Box,
    item: RefCell<dl::Item>,
    look: Cell<Option<(usize, bool, bool)>>,
    press_at: Cell<Option<(f64, f64)>>,
    press_in_slide: Cell<bool>,
    press_outdated: Cell<bool>,
    dragging: Cell<bool>,
    tip: RefCell<Option<Rc<Tip>>>,
}

impl DockItem {
    fn update(&self, item: dl::Item) {
        let look = (item.windows.len().min(2), item.focused(), item.urgent());
        *self.item.borrow_mut() = item; // sempre: a lista de janelas mostra os títulos atuais
        if self.look.get() == Some(look) {
            return; // um título mudando gera uma leitura atrás da outra
        }
        self.look.set(Some(look));
        while let Some(child) = self.dots.first_child() {
            self.dots.remove(&child);
        }
        for _ in 0..look.0 {
            // um ponto: uma janela; dois: várias
            let dot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            dot.add_css_class("dot");
            self.dots.append(&dot);
        }
        for (name, on) in [("focused", look.1), ("urgent", look.2)] {
            if on {
                self.widget.add_css_class(name);
            } else {
                self.widget.remove_css_class(name);
            }
        }
    }

    fn pinned(&self) -> bool {
        self.item.borrow().pinned
    }

    fn remove_tip(&self) {
        if let Some(tip) = self.tip.borrow_mut().take() {
            tip.remove();
        }
    }
}

/// Uma coluna de item: o ícone e a fileira de pontos embaixo.
fn item_column(icon: &gtk4::Widget) -> (gtk4::Box, gtk4::Box) {
    let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    widget.add_css_class("dock-item");
    widget.append(icon);
    let dots = gtk4::Box::new(gtk4::Orientation::Horizontal, 3);
    dots.set_halign(gtk4::Align::Center);
    dots.add_css_class("dots");
    widget.append(&dots);
    (widget, dots)
}

// --- A dock de um monitor ---

/// A dock de um monitor: a janela e a faixa na borda que a faz aparecer.
pub struct Bar {
    app: Weak<App>,
    /// Esta própria dock, para os timers criados pelas funções que só recebem &self
    me: RefCell<Weak<Bar>>,
    monitor: gdk::Monitor,
    config: dl::Config,
    window: gtk4::Window,
    root: gtk4::Box,
    items: gtk4::Box,
    /// O mesmo widget de items: a animação do arrasto
    drag_view: DragView,
    hotspot: Option<(gtk4::Window, gtk4::Box)>,
    dock_motion: gtk4::EventControllerMotion,
    launcher_tip: RefCell<Option<Rc<Tip>>>,
    buttons: RefCell<HashMap<String, Rc<DockItem>>>,
    menu: RefCell<Option<gtk4::Popover>>,
    menu_opened: Cell<i64>,
    /// Fechou e o finish_menu ainda vai rodar: não está preso
    menu_closing: Cell<bool>,
    dragging: Cell<bool>,
    drag_moved: Cell<i64>,
    pending: RefCell<Option<Vec<dl::Item>>>,
    needs_reset: Cell<bool>,
    shown_count: Cell<usize>,
    hover_dock: Cell<bool>,
    hover_hotspot: Cell<bool>,
    /// O ponteiro acabou de sair da faixa da borda: na hora de esconder, confere se ele foi para a dock
    left_hotspot: Cell<bool>,
    is_overlapped: Cell<bool>,
    hide_timer: RefCell<Option<glib::SourceId>>,
    slide: RefCell<dl::Slide>,
    slide_size: Cell<i32>,
    frame_timer: RefCell<Option<glib::SourceId>>,
    /// Recriada com a dock anterior visível: não desliza outra vez
    restoring: Cell<bool>,
}

fn make_surface(monitor: &gdk::Monitor, namespace: &str) -> gtk4::Window {
    let window = gtk4::Window::new();
    window.init_layer_shell();
    // Acima das janelas e abaixo do OVERLAY, onde ficam a grade de apps, os popups e os toasts
    window.set_layer(Layer::Top);
    window.set_monitor(monitor);
    window.set_anchor(Edge::Bottom, true);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_namespace(namespace);
    window
}

impl Bar {
    fn new(app: &Rc<App>, monitor: &gdk::Monitor, restoring: bool) -> Rc<Bar> {
        let config = app.config.borrow().clone();
        // No "always" a dock nunca se esconde: aparece direto, sem animação
        let slide = dl::Slide { progress: if config.mode == "always" { 0.0 } else { 1.0 }, ..Default::default() };
        let window = make_surface(monitor, "sway-dock");
        if config.mode == "always" {
            window.auto_exclusive_zone_enable();
        } else {
            window.set_exclusive_zone(-1);
        }
        let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        root.set_widget_name("dock");
        let drag_view = DragView::default();
        let items = drag_view.clone().upcast::<gtk4::Box>();
        root.append(&items);
        window.set_child(Some(&root));
        let dock_motion = gtk4::EventControllerMotion::new();
        window.add_controller(dock_motion.clone());

        let hotspot = dl::uses_hotspot(&config.mode).then(|| {
            let window = make_surface(monitor, "sway-dock-hotspot");
            window.add_css_class("hotspot");
            window.set_exclusive_zone(-1);
            let area = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            area.set_size_request(1, HOTSPOT_HEIGHT);
            window.set_child(Some(&area));
            (window, area)
        });

        let bar = Rc::new(Bar {
            app: Rc::downgrade(app),
            me: RefCell::new(Weak::new()),
            monitor: monitor.clone(),
            config,
            window,
            root,
            items,
            drag_view,
            hotspot,
            dock_motion,
            launcher_tip: RefCell::new(None),
            buttons: RefCell::new(HashMap::new()),
            menu: RefCell::new(None),
            menu_opened: Cell::new(0),
            menu_closing: Cell::new(false),
            dragging: Cell::new(false),
            drag_moved: Cell::new(0),
            pending: RefCell::new(None),
            needs_reset: Cell::new(false),
            shown_count: Cell::new(0),
            hover_dock: Cell::new(false),
            hover_hotspot: Cell::new(false),
            left_hotspot: Cell::new(false),
            is_overlapped: Cell::new(false),
            hide_timer: RefCell::new(None),
            slide: RefCell::new(slide),
            slide_size: Cell::new(0),
            frame_timer: RefCell::new(None),
            restoring: Cell::new(restoring),
        });
        *bar.me.borrow_mut() = Rc::downgrade(&bar);
        if !bar.config.launcher.is_empty() {
            let launcher = bar.make_launcher();
            bar.root.append(&launcher);
        }
        bar.watch_hover(&bar.dock_motion, false);
        bar.watch_escape();
        if let Some((hotspot, _)) = &bar.hotspot {
            let motion = gtk4::EventControllerMotion::new();
            hotspot.add_controller(motion.clone());
            bar.watch_hover(&motion, true);
            hotspot.set_visible(true);
        }
        bar
    }

    /// Com o menu aberto, o teclado está na camada da dock, e não no popover: o Esc chega aqui
    fn watch_escape(self: &Rc<Self>) {
        let keys = gtk4::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(bar) = weak.upgrade() else { return glib::Propagation::Proceed };
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            let menu = bar.menu.borrow().clone();
            match menu {
                Some(menu) => {
                    menu.popdown();
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(keys);
    }

    fn watch_hover(self: &Rc<Self>, motion: &gtk4::EventControllerMotion, hotspot: bool) {
        let weak = Rc::downgrade(self);
        motion.connect_enter(move |controller, _, _| {
            let Some(bar) = weak.upgrade() else { return };
            if bar.dragging.get() && !controller.current_event_state().contains(gdk::ModifierType::BUTTON1_MASK) {
                bar.end_stuck(dl::Stuck::Drag, "the pointer came back without the button pressed");
            }
            bar.hover_flag(hotspot).set(true);
            bar.update_visibility();
        });
        let weak = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            let Some(bar) = weak.upgrade() else { return };
            bar.hover_flag(hotspot).set(false);
            bar.left_hotspot.set(hotspot);
            bar.update_visibility();
        });
    }

    fn hover_flag(&self, hotspot: bool) -> &Cell<bool> {
        if hotspot {
            &self.hover_hotspot
        } else {
            &self.hover_dock
        }
    }

    fn make_launcher(self: &Rc<Self>) -> gtk4::Box {
        let (widget, _) = item_column(&make_glyph(ICON_APPS, self.config.icon_size as i32));
        *self.launcher_tip.borrow_mut() = Some(Tip::new(self, &widget, &tr("Applications")));
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        let weak = Rc::downgrade(self);
        click.connect_released(move |gesture, _, x, y| {
            let Some(widget) = gesture.widget() else { return };
            let Some(bar) = weak.upgrade() else { return };
            bar.hide_tips();
            if x >= 0.0 && y >= 0.0 && x < widget.width() as f64 && y < widget.height() as f64 {
                bar.on_launcher();
            }
        });
        widget.add_controller(click);
        widget
    }

    fn hide_tips(&self) {
        for button in self.buttons.borrow().values() {
            if let Some(tip) = button.tip.borrow().as_ref() {
                tip.hide();
            }
        }
        if let Some(tip) = self.launcher_tip.borrow().as_ref() {
            tip.hide();
        }
    }

    fn on_launcher(&self) {
        match shlex::split(&expand_home(&self.config.launcher)) {
            Some(mut argv) if !argv.is_empty() => {
                // "sway-drawer" (o padrão) ou o ~/.local/bin/sway-drawer, padrão antigo que pode estar no
                // config: a grade de apps dos dotfiles fora da sessão do pacote, senão o swaydesk-drawer
                if argv[0] == "sway-drawer" || argv[0].ends_with("/.local/bin/sway-drawer") {
                    argv[0] = swaydesk_common::paths::component("sway-drawer", "swaydesk-drawer").to_string_lossy().into_owned();
                }
                // Numa sessão própria: o app aberto não conta como auxiliar da dock no popup "Sistema"; com a dock
                // num serviço do usuário, num escopo próprio
                if let Err(err) = cmd::spawn_app(&argv[0], &argv[1..]) {
                    warn(&format!("could not run {}: {err}", argv[0]));
                }
            }
            Some(_) => {}
            None => warn("invalid launcher: unbalanced quotes"),
        }
        self.action_done();
    }

    fn destroy(&self) {
        self.cancel_hide();
        self.cancel_slide();
        if let Some(menu) = self.menu.borrow_mut().take() {
            menu.unparent();
        }
        for button in self.buttons.borrow().values() {
            button.remove_tip();
        }
        if let Some(tip) = self.launcher_tip.borrow_mut().take() {
            tip.remove();
        }
        self.window.destroy();
        if let Some((hotspot, _)) = &self.hotspot {
            hotspot.destroy();
        }
    }

    // --- itens ---

    fn set_items(self: &Rc<Self>, items: Vec<dl::Item>) {
        // Recriar um item fecharia o menu ou soltaria o arrasto
        if self.menu.borrow().is_some() || self.dragging.get() {
            *self.pending.borrow_mut() = Some(items);
        } else {
            self.render(items);
        }
    }

    fn make_item(self: &Rc<Self>, item: dl::Item) -> Rc<DockItem> {
        let info = desktop_info(&item.key);
        let icon = make_icon(info.as_ref(), &item.key, self.config.icon_size as i32);
        let (widget, dots) = item_column(&icon);
        // O nome só muda se os apps instalados mudarem, e aí os itens são recriados
        let name = info.map(|i| i.display_name().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| item.key.clone());
        let entry = Rc::new(DockItem {
            widget,
            dots,
            item: RefCell::new(item.clone()),
            look: Cell::new(None),
            press_at: Cell::new(None),
            press_in_slide: Cell::new(false),
            press_outdated: Cell::new(false),
            dragging: Cell::new(false),
            tip: RefCell::new(None),
        });
        entry.update(item);
        *entry.tip.borrow_mut() = Some(Tip::new(self, &entry.widget, &name));
        self.connect_item(&entry);
        entry
    }

    fn connect_item(self: &Rc<Self>, entry: &Rc<DockItem>) {
        let drag = gtk4::GestureDrag::new();
        drag.set_button(1);
        let (bar, it) = (Rc::downgrade(self), Rc::downgrade(entry));
        drag.connect_drag_begin(move |_, x, y| {
            let (Some(bar), Some(it)) = (bar.upgrade(), it.upgrade()) else { return };
            it.press_at.set(Some((x, y)));
            it.dragging.set(false);
            bar.hide_tips();
            // Com a dock em movimento, o press traz a posição de antes: o Sway só informa a certa no
            // movimento seguinte, e esse salto não pode virar arrasto
            let sliding = bar.sliding();
            it.press_in_slide.set(sliding);
            it.press_outdated.set(sliding);
        });
        let (bar, it) = (Rc::downgrade(self), Rc::downgrade(entry));
        drag.connect_drag_update(move |gesture, dx, dy| {
            let (Some(bar), Some(it)) = (bar.upgrade(), it.upgrade()) else { return };
            let Some((px, py)) = it.press_at.get() else { return };
            let Some((sx, sy)) = gesture.start_point() else { return };
            let (x, y) = (sx + dx, sy + dy);
            if it.press_outdated.replace(false) {
                it.press_at.set(Some((x, y)));
                return;
            }
            if !it.dragging.get() {
                if !it.pinned() || !it.widget.drag_check_threshold(px as i32, py as i32, x as i32, y as i32) {
                    return;
                }
                it.dragging.set(true);
                bar.start_drag(&it, px);
            }
            if let Some(point) = it.widget.compute_point(&bar.items, &graphene::Point::new(x as f32, y as f32)) {
                bar.drag_to(&it, point.x() as f64);
            }
        });
        let (bar, it) = (Rc::downgrade(self), Rc::downgrade(entry));
        drag.connect_drag_end(move |gesture, dx, dy| {
            let (Some(bar), Some(it)) = (bar.upgrade(), it.upgrade()) else { return };
            if it.press_at.take().is_none() {
                return;
            }
            if it.dragging.replace(false) {
                bar.end_drag();
                return;
            }
            let Some((sx, sy)) = gesture.start_point() else { return };
            let (x, y) = (sx + dx, sy + dy);
            let (width, height) = (it.widget.width() as f64, it.widget.height() as f64);
            // No slide, o y pode ter saltado para fora do item
            if (0.0..width).contains(&x) && (it.press_in_slide.get() || (0.0..height).contains(&y)) {
                bar.activate(&it);
            }
        });
        let (bar, it) = (Rc::downgrade(self), Rc::downgrade(entry));
        drag.connect_cancel(move |_, _| {
            // O GTK desistiu do gesto (o botão foi solto sem o evento chegar, por exemplo): termina sem
            // ativar o item
            let (Some(bar), Some(it)) = (bar.upgrade(), it.upgrade()) else { return };
            it.press_at.set(None);
            if it.dragging.replace(false) {
                bar.end_drag();
            }
        });
        entry.widget.add_controller(drag);

        let right = gtk4::GestureClick::new();
        right.set_button(3);
        let (bar, it) = (Rc::downgrade(self), Rc::downgrade(entry));
        right.connect_pressed(move |_, _, _, _| {
            let (Some(bar), Some(it)) = (bar.upgrade(), it.upgrade()) else { return };
            if it.press_at.get().is_none() {
                // o esquerdo continua apertado: nada
                bar.hide_tips();
                bar.open_context_menu(&it);
            }
        });
        entry.widget.add_controller(right);
    }

    fn render(self: &Rc<Self>, items: Vec<dl::Item>) {
        self.pending.borrow_mut().take();
        if self.needs_reset.replace(false) {
            // Os apps instalados mudaram: ícones e nomes podem ser outros
            for (_, button) in self.buttons.borrow_mut().drain() {
                button.remove_tip();
                self.items.remove(&button.widget);
            }
        }
        {
            let mut buttons = self.buttons.borrow_mut();
            let wanted: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
            buttons.retain(|key, button| {
                let keep = wanted.contains(&key.as_str());
                if !keep {
                    button.remove_tip();
                    self.items.remove(&button.widget);
                }
                keep
            });
        }
        let before = children(&self.items);
        let mut ordered = Vec::with_capacity(items.len());
        for item in items {
            let existing = self.buttons.borrow().get(&item.key).cloned();
            let button = match existing {
                Some(button) => {
                    button.update(item);
                    button
                }
                None => {
                    let key = item.key.clone();
                    let button = self.make_item(item);
                    self.items.append(&button.widget);
                    self.buttons.borrow_mut().insert(key, button.clone());
                    button
                }
            };
            ordered.push(button.widget.clone().upcast::<gtk4::Widget>());
        }
        let count = self.buttons.borrow().len();
        if before == ordered && count == self.shown_count.get() {
            return; // nada mudou de lugar: a janela fica como está
        }
        self.shown_count.set(count);
        let mut previous: Option<gtk4::Widget> = None;
        for widget in &ordered {
            self.items.reorder_child_after(widget, previous.as_ref());
            previous = Some(widget.clone());
        }
        self.window.set_default_size(1, 1); // sem isso a janela não encolhe quando um item sai
        self.sync_hotspot();
        self.area_changed();
    }

    /// Se a dock está à vista ou subindo: a área que a grade de apps deixa livre para os cliques.
    fn shown(&self) -> bool {
        self.window.is_visible() && self.slide.borrow().target == 0.0
    }

    /// A dock à vista, ou a faixa da borda que a traz (escondida): com a grade aberta, o ponteiro na
    /// faixa chama a dock por cima da grade. None sem nenhuma das duas.
    fn area(&self) -> Option<dl::Rect> {
        let (width, height) = self.natural_size();
        let geometry = monitor_rect(&self.monitor);
        if self.shown() {
            Some(dl::dock_rect(geometry, width as i64, height as i64))
        } else {
            self.hotspot.as_ref().map(|_| dl::dock_rect(geometry, width.max(1) as i64, HOTSPOT_HEIGHT as i64))
        }
    }

    fn area_changed(&self) {
        if let Some(app) = self.app.upgrade() {
            app.publish_area();
        }
    }

    fn natural_size(&self) -> (i32, i32) {
        let (_, natural) = self.root.preferred_size();
        (natural.width(), natural.height())
    }

    fn sync_hotspot(&self) {
        if let Some((hotspot, area)) = &self.hotspot {
            let width = self.natural_size().0.max(1);
            area.set_size_request(width, HOTSPOT_HEIGHT);
            hotspot.set_default_size(width, HOTSPOT_HEIGHT);
        }
    }

    // --- visibilidade ---

    fn set_overlap(&self, windows: &[dl::Window], outputs: &HashMap<String, (dl::Rect, Option<String>)>) {
        // A geometria muda sem o monitor sair (escala, posição): sempre a atual
        let geometry = monitor_rect(&self.monitor);
        let name = dl::output_at(outputs, geometry).unwrap_or_default();
        let workspace = outputs.get(&name).and_then(|o| o.1.clone()).unwrap_or_default();
        let (width, height) = self.natural_size();
        let rect = dl::dock_rect(geometry, width as i64, height as i64);
        self.is_overlapped.set(!name.is_empty() && dl::overlapped(rect, windows, &name, &workspace));
        self.update_visibility();
    }

    fn wanted(&self) -> bool {
        dl::should_show(
            &self.config.mode,
            self.hover_dock.get() || self.hover_hotspot.get(),
            self.menu.borrow().is_some(),
            self.dragging.get(),
            self.is_overlapped.get(),
        )
    }

    fn update_visibility(&self) {
        let restoring = self.restoring.replace(false);
        if self.wanted() {
            self.cancel_hide();
            if !self.window.is_visible() {
                if restoring {
                    self.slide.borrow_mut().progress = 0.0;
                }
                self.show_bar();
            } else if self.slide.borrow().target != 0.0 {
                self.slide_to(0.0); // estava descendo: sobe de onde parou
                self.area_changed();
            }
        } else if self.window.is_visible() && self.hide_timer.borrow().is_none() && self.slide.borrow().target != 1.0 {
            let weak = self.self_weak();
            let delay = Duration::from_millis(self.config.hide_delay_ms.max(0) as u64);
            let timer = glib::timeout_add_local_once(delay, move || {
                if let Some(bar) = weak.upgrade() {
                    bar.hide_timer.borrow_mut().take();
                    if !bar.wanted() && bar.left_hotspot.get() && bar.pointer_on_dock() {
                        // O ponteiro passou da faixa da borda para a dock enquanto ela subia, e o GTK nem
                        // sempre entrega essa entrada (visto na VM): a dock descia por baixo do ponteiro, e
                        // o clique caía no que estivesse atrás. Sem a entrada, a saída também pode não vir:
                        // em vez de marcar o hover, confere de novo no próximo disparo
                        bar.update_visibility();
                    } else if !bar.wanted() {
                        bar.hide_bar();
                    }
                }
            });
            *self.hide_timer.borrow_mut() = Some(timer);
        }
    }

    /// Se o ponteiro está na janela da dock, pelo que o seat sabe (o último enter do Wayland).
    fn pointer_on_dock(&self) -> bool {
        let Some(surface) = self.window.surface() else { return false };
        let pointer = gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.pointer());
        pointer.and_then(|p| p.surface_at_position().0).is_some_and(|under| under == surface)
    }

    fn self_weak(&self) -> Weak<Bar> {
        self.me.borrow().clone()
    }

    fn cancel_hide(&self) {
        if let Some(timer) = self.hide_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    // --- animação ---

    fn animations(&self) -> bool {
        self.config.animation_ms > 0 && gtk4::Settings::default().is_none_or(|s| s.is_gtk_enable_animations())
    }

    /// Margem negativa: a dock desce até sobrar uma faixa na tela (dl::slide_margin diz por que), e o fim da
    /// descida a tira do mapa.
    fn apply_slide(&self) {
        let margin = dl::slide_margin(self.slide.borrow().progress, self.slide_size.get());
        self.window.set_margin(Edge::Bottom, margin);
    }

    fn sliding(&self) -> bool {
        self.slide.borrow().running
    }

    fn show_bar(&self) {
        // A altura antes de mostrar: a margem negativa já deixa a janela quase fora da tela
        self.slide_size.set(self.natural_size().1);
        if self.slide.borrow().progress > 0.0 {
            self.apply_slide();
        }
        self.window.set_visible(true);
        self.slide_to(0.0);
        self.area_changed();
    }

    fn hide_bar(&self) {
        self.left_hotspot.set(false);
        self.hide_tips();
        self.slide_size.set(self.natural_size().1);
        self.slide_to(1.0);
        self.area_changed();
    }

    fn slide_to(&self, target: f64) {
        if !self.animations() {
            self.cancel_slide(); // a configuração pode ter mudado com uma animação em andamento
            {
                let mut slide = self.slide.borrow_mut();
                slide.progress = target;
                slide.target = target;
                slide.running = false;
            }
            self.apply_slide();
            self.finish_slide();
        } else if self.slide.borrow_mut().start(target, now_ms()) {
            // Timer, e não frame clock: com a janela quase toda fora da tela o GTK não gera quadros. O
            // timer do GLib não depende de pintura
            if self.frame_timer.borrow().is_none() {
                let weak = self.self_weak();
                let timer = glib::timeout_add_local(FRAME, move || {
                    let Some(bar) = weak.upgrade() else { return glib::ControlFlow::Break };
                    let running = bar.slide.borrow_mut().step(now_ms(), bar.config.animation_ms);
                    bar.apply_slide();
                    if running {
                        return glib::ControlFlow::Continue;
                    }
                    bar.frame_timer.borrow_mut().take();
                    bar.finish_slide();
                    glib::ControlFlow::Break
                });
                *self.frame_timer.borrow_mut() = Some(timer);
            }
        } else {
            self.finish_slide();
        }
    }

    fn finish_slide(&self) {
        if self.slide.borrow().progress >= 1.0 {
            self.window.set_visible(false); // só some de verdade no último quadro
        }
    }

    fn cancel_slide(&self) {
        if let Some(timer) = self.frame_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    /// Depois de uma ação, some mesmo com o ponteiro em cima, como no nwg-dock.
    fn action_done(&self) {
        self.hover_dock.set(false);
        self.hover_hotspot.set(false);
        self.left_hotspot.set(false); // some mesmo com o ponteiro em cima
        self.update_visibility();
    }

    // --- cliques ---

    fn activate(self: &Rc<Self>, it: &Rc<DockItem>) {
        hide_drawer();
        let item = it.item.borrow().clone();
        match item.windows.len() {
            0 => {
                launch(&item.key);
                self.action_done();
            }
            1 => self.focus(&item.windows[0]),
            _ => self.open_window_list(it),
        }
    }

    fn focus(&self, window: &dl::Window) {
        sway(&format!("[con_id={}] focus", window.id));
        self.action_done();
    }

    fn open_window_list(self: &Rc<Self>, it: &Rc<DockItem>) {
        let windows = it.item.borrow().windows.clone();
        let entries: Vec<MenuEntry> = windows
            .into_iter()
            .map(|w| {
                let title = if w.title.is_empty() { w.app_id.clone() } else { w.title.clone() };
                let text = format!("{title}  \u{b7}  {}", w.workspace);
                MenuEntry::Action(text, Box::new(move |bar: &Rc<Bar>| bar.focus(&w)))
            })
            .collect();
        self.popup(entries, it);
    }

    fn open_context_menu(self: &Rc<Self>, it: &Rc<DockItem>) {
        hide_drawer(); // o menu abre na camada da dock, que a grade cobre
        let item = it.item.borrow().clone();
        let mut entries = Vec::new();
        let key = item.key.clone();
        entries.push(MenuEntry::Action(
            if item.pinned { tr("Unpin") } else { tr("Pin") },
            Box::new(move |bar: &Rc<Bar>| {
                if let Some(app) = bar.app.upgrade() {
                    app.toggle_pin(&key);
                }
            }),
        ));
        if desktop_info(&item.key).is_some() {
            let key = item.key.clone();
            entries.push(MenuEntry::Action(
                tr("New window"),
                Box::new(move |bar: &Rc<Bar>| {
                    launch(&key);
                    bar.action_done();
                }),
            ));
        }
        if !item.windows.is_empty() {
            entries.push(MenuEntry::Separator);
            let count = item.windows.len();
            let text = if count == 1 { tr("Close") } else { tr("Close all (%d)").replace("%d", &count.to_string()) };
            let windows = item.windows.clone();
            entries.push(MenuEntry::Action(
                text,
                Box::new(move |bar: &Rc<Bar>| {
                    for window in &windows {
                        sway(&format!("[con_id={}] kill", window.id));
                    }
                    bar.action_done();
                }),
            ));
        }
        self.popup(entries, it);
    }

    fn popup(self: &Rc<Self>, entries: Vec<MenuEntry>, it: &Rc<DockItem>) {
        if self.menu.borrow().is_some() {
            return;
        }
        self.hide_tips();
        let menu = gtk4::Popover::new();
        menu.add_css_class("menu");
        menu.set_has_arrow(false);
        menu.set_position(gtk4::PositionType::Top);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        for entry in entries {
            match entry {
                MenuEntry::Separator => column.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal)),
                MenuEntry::Action(text, action) => {
                    let button = gtk4::Button::with_label(&text);
                    if let Some(label) = button.child().and_downcast::<gtk4::Label>() {
                        label.set_xalign(0.0);
                        label.set_max_width_chars(MENU_TITLE_CHARS);
                        label.set_ellipsize(pango::EllipsizeMode::End);
                    }
                    let weak = Rc::downgrade(self);
                    let action = RefCell::new(Some(action));
                    button.connect_clicked(move |button| {
                        let Some(bar) = weak.upgrade() else { return };
                        // Fecha antes: a ação pode refazer os itens, e o menu ainda conta como aberto até o
                        // finish_menu, que roda depois
                        if let Some(menu) = button.ancestor(gtk4::Popover::static_type()).and_downcast::<gtk4::Popover>() {
                            menu.popdown();
                        }
                        if let Some(action) = action.borrow_mut().take() {
                            action(&bar);
                        }
                    });
                    column.append(&button);
                }
            }
        }
        menu.set_child(Some(&column));
        menu.set_parent(&it.widget);
        let weak = Rc::downgrade(self);
        menu.connect_closed(move |_| {
            let Some(bar) = weak.upgrade() else { return };
            bar.menu_closing.set(true);
            let weak = Rc::downgrade(&bar);
            glib::idle_add_local_once(move || {
                if let Some(bar) = weak.upgrade() {
                    bar.finish_menu();
                }
            });
        });
        *self.menu.borrow_mut() = Some(menu.clone());
        self.menu_opened.set(now_ms());
        self.menu_closing.set(false);
        // O Sway não dá o teclado aos popups: o Esc ia para a janela focada, o menu ficava aberto e o clique
        // seguinte na dock só o fechava. A camada da dock pega o teclado (exclusivo só com o menu aberto; o
        // finish_menu devolve) e o watch_escape fecha o menu. O menu abre depois que o teclado chegou:
        // aberto antes, o grab do popup no wlroots segura o teclado onde estava, na janela focada
        self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        self.popup_when_focused(&menu);
        if let Some(app) = self.app.upgrade() {
            app.watch_stuck();
        }
    }

    /// Abre o menu quando a janela da dock recebe o teclado, ou depois de KEYBOARD_WAIT_MS se ele não vier
    fn popup_when_focused(self: &Rc<Self>, menu: &gtk4::Popover) {
        if self.window.is_active() {
            self.show_menu(menu);
            return;
        }
        struct Wait {
            handler: Option<glib::SignalHandlerId>,
            timer: Option<glib::SourceId>,
        }
        let wait = Rc::new(RefCell::new(Wait { handler: None, timer: None }));
        let open = {
            let (weak, menu, wait) = (Rc::downgrade(self), menu.clone(), wait.clone());
            Rc::new(move || {
                let (handler, timer) = {
                    let mut wait = wait.borrow_mut();
                    (wait.handler.take(), wait.timer.take())
                };
                let Some(handler) = handler else { return }; // já abriu
                let Some(bar) = weak.upgrade() else { return };
                bar.window.disconnect(handler);
                if let Some(timer) = timer {
                    timer.remove();
                }
                // O menu pode ter sido desfeito enquanto esperava
                if bar.menu.borrow().as_ref() == Some(&menu) {
                    bar.show_menu(&menu);
                }
            })
        };
        let open_on_focus = open.clone();
        let handler = self.window.connect_is_active_notify(move |window| {
            if window.is_active() {
                open_on_focus();
            }
        });
        let wait_timer = wait.clone();
        let timer = glib::timeout_add_local_once(Duration::from_millis(KEYBOARD_WAIT_MS), move || {
            wait_timer.borrow_mut().timer = None; // já disparou: não há o que remover
            open();
        });
        *wait.borrow_mut() = Wait { handler: Some(handler), timer: Some(timer) };
    }

    /// Mostra o menu. Se o GDK não consegue apresentar o popup (o Sway o dispensou antes de configurá-lo,
    /// por exemplo), o GtkPopover fica visível e fora da tela, sem emitir o "closed": a dock ficaria com o
    /// teclado exclusivo até a checagem do estado preso. Fecha na hora, pelo caminho normal.
    fn show_menu(self: &Rc<Self>, menu: &gtk4::Popover) {
        menu.popup();
        if menu.is_mapped() {
            return;
        }
        warn(&format!("the dock menu did not open; released. {}", self.describe()));
        menu.popdown(); // emite o "closed", que agenda o finish_menu
        if !self.menu_closing.get() && self.menu.borrow().as_ref() == Some(menu) {
            self.finish_menu();
        }
    }

    fn finish_menu(self: &Rc<Self>) {
        self.menu_closing.set(false);
        if let Some(menu) = self.menu.borrow_mut().take() {
            menu.unparent();
        }
        // O Sway devolve o teclado à janela que o tinha
        self.window.set_keyboard_mode(KeyboardMode::None);
        // Com o menu aberto, a saída do ponteiro pode não ter chegado: o que se sabia dele ficou velho
        let over = self.dock_motion.contains_pointer();
        self.hover_dock.set(over);
        self.hover_hotspot.set(false);
        if !over {
            // pelo mesmo motivo, o item continuaria destacado ao reaparecer
            for button in self.buttons.borrow().values() {
                button.widget.unset_state_flags(gtk4::StateFlags::PRELIGHT);
            }
        }
        let pending = self.pending.borrow_mut().take();
        if let Some(items) = pending {
            self.render(items);
        }
        self.update_visibility();
    }

    // --- arrasto ---

    fn pinned_items(&self) -> Vec<Rc<DockItem>> {
        let buttons = self.buttons.borrow();
        children(&self.items)
            .into_iter()
            .filter_map(|child| buttons.values().find(|b| b.widget.upcast_ref::<gtk4::Widget>() == &child).cloned())
            .filter(|b| b.pinned())
            .collect()
    }

    /// O arrasto de `it` começou, pego no ponto `grab` (x no item).
    fn start_drag(&self, it: &Rc<DockItem>, grab: f64) {
        self.dragging.set(true);
        self.drag_moved.set(now_ms());
        if self.animations() {
            self.drag_view.lift(it.widget.upcast_ref(), grab);
        }
        self.update_visibility();
        if let Some(app) = self.app.upgrade() {
            app.watch_stuck();
        }
    }

    fn drag_to(&self, it: &Rc<DockItem>, x: f64) {
        self.drag_moved.set(now_ms());
        self.drag_view.follow(x);
        let pinned = self.pinned_items();
        // O lugar de cada um: a borda (bounds, com o padding; o width() do GTK 4 o deixa de fora, e o
        // lugar ficava 8 px menor) mais a margem do CSS
        let widths: Vec<f64> = pinned
            .iter()
            .map(|b| b.widget.compute_bounds(&self.items).map_or(b.widget.width() as f64, |r| r.width() as f64) + ITEM_MARGIN)
            .collect();
        let Some(current) = pinned.iter().position(|b| Rc::ptr_eq(b, it)) else { return };
        let target = dl::drag_index(x, &widths, current);
        if current == target {
            return;
        }
        let others: Vec<&Rc<DockItem>> = pinned.iter().filter(|b| !Rc::ptr_eq(b, it)).collect();
        let sibling = if target == 0 { None } else { others.get(target - 1).map(|b| b.widget.clone()) };
        self.items.reorder_child_after(&it.widget, sibling.as_ref());
    }

    fn end_drag(self: &Rc<Self>) {
        self.dragging.set(false);
        self.drag_view.release(); // assenta no lugar, também num arrasto cancelado ou preso
        let order: Vec<String> = self.pinned_items().iter().map(|b| b.item.borrow().key.clone()).collect();
        if let Some(app) = self.app.upgrade() {
            app.set_pin_order(order);
        }
        let pending = self.pending.borrow_mut().take();
        if let Some(items) = pending {
            self.render(items);
        }
        self.update_visibility();
    }

    // --- estado preso ---

    /// Um menu que não chegou à tela ou um arrasto cujo fim se perdeu deixavam a dock visível e com os
    /// ícones de antes.
    fn check_stuck(self: &Rc<Self>, button1_down: bool) -> Option<dl::Stuck> {
        let now = now_ms();
        let (menu_age, mapped) = match self.menu.borrow().as_ref() {
            None => (None, false),
            Some(menu) => (Some((now - self.menu_opened.get()).max(0) as u64), self.menu_closing.get() || menu.is_mapped()),
        };
        let drag_idle = self.dragging.get().then(|| (now - self.drag_moved.get()).max(0) as u64);
        let stuck = dl::stuck_state(menu_age, mapped, drag_idle, button1_down);
        match stuck {
            Some(dl::Stuck::Menu) => self.end_stuck(dl::Stuck::Menu, "the menu is not on screen"),
            Some(dl::Stuck::Drag) => {
                self.end_stuck(dl::Stuck::Drag, if button1_down { "without movement" } else { "without the button pressed" })
            }
            None => {}
        }
        stuck
    }

    fn end_stuck(self: &Rc<Self>, what: dl::Stuck, why: &str) {
        let name = if what == dl::Stuck::Menu { "menu" } else { "drag" };
        warn(&format!("stuck state ({name}): {why}; released. {}", self.describe()));
        if what == dl::Stuck::Menu {
            self.finish_menu();
            return;
        }
        for button in self.buttons.borrow().values() {
            button.press_at.set(None);
            button.dragging.set(false);
        }
        self.end_drag();
    }

    fn describe(&self) -> String {
        let menu = match self.menu.borrow().as_ref() {
            None => "no",
            Some(m) if m.is_mapped() => "on screen",
            Some(_) => "off screen",
        };
        let slide = self.slide.borrow();
        let (shifted, animating) = self.drag_view.state();
        format!(
            "hover_dock={} hover_hotspot={} menu={menu} dragging={} overlapped={} visible={} size={}x{} \
             slide={:.2}->{:.2} pending={} shifted={shifted} drag_animating={animating}",
            self.hover_dock.get(),
            self.hover_hotspot.get(),
            self.dragging.get(),
            self.is_overlapped.get(),
            self.window.is_visible(),
            self.window.width(),
            self.window.height(),
            slide.progress,
            slide.target,
            self.pending.borrow().is_some()
        )
    }
}

enum MenuEntry {
    Action(String, Box<dyn Fn(&Rc<Bar>)>),
    Separator,
}

fn children(container: &gtk4::Box) -> Vec<gtk4::Widget> {
    let mut found = Vec::new();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        found.push(widget);
    }
    found
}

// --- O processo ---

pub struct App {
    me: RefCell<Weak<App>>,
    main_loop: glib::MainLoop,
    exit_code: Cell<u8>,
    config: RefCell<dl::Config>,
    pins: RefCell<Vec<String>>,
    pins_unreadable: Cell<bool>,
    /// Sem o arquivo de fixados: a dock mostra os default_pins e não grava até o usuário mudar algo
    pins_default: Cell<bool>,
    windows: RefCell<Vec<dl::Window>>,
    outputs: RefCell<HashMap<String, (dl::Rect, Option<String>)>>,
    bars: RefCell<Vec<Rc<Bar>>>,
    /// As docks já refeitas: só referências fracas, que o dump confere (uma viva é um closure que
    /// ainda segura a dock, e a janela dela nunca seria solta)
    retired: RefCell<Vec<Weak<Bar>>>,
    retired_total: Cell<usize>,
    bars_dirty: Cell<bool>,
    poll_timer: RefCell<Option<glib::SourceId>>,
    stuck_timer: RefCell<Option<glib::SourceId>>,
    refresh_scheduled: Cell<bool>,
    refresh_running: Cell<bool>,
    refresh_again: Cell<bool>,
    apps: Apps,
    provider: gtk4::CssProvider,
    watched_monitors: RefCell<Vec<gdk::Monitor>>,
    watcher: RefCell<Option<Child>>,
    area_queued: Cell<bool>,
    _app_monitor: gio::AppInfoMonitor,
}

impl App {
    pub fn new(main_loop: &glib::MainLoop) -> Rc<App> {
        let provider = gtk4::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(true);
        }
        let app = Rc::new(App {
            me: RefCell::new(Weak::new()),
            main_loop: main_loop.clone(),
            exit_code: Cell::new(0),
            config: RefCell::new(dl::Config::default()),
            pins: RefCell::new(vec![]),
            pins_unreadable: Cell::new(false),
            pins_default: Cell::new(false),
            windows: RefCell::new(vec![]),
            outputs: RefCell::new(HashMap::new()),
            bars: RefCell::new(vec![]),
            retired: RefCell::new(vec![]),
            retired_total: Cell::new(0),
            bars_dirty: Cell::new(true),
            poll_timer: RefCell::new(None),
            stuck_timer: RefCell::new(None),
            refresh_scheduled: Cell::new(false),
            refresh_running: Cell::new(false),
            refresh_again: Cell::new(false),
            apps: Apps::new(),
            provider,
            watched_monitors: RefCell::new(vec![]),
            watcher: RefCell::new(None),
            area_queued: Cell::new(false),
            _app_monitor: gio::AppInfoMonitor::get(),
        });
        *app.me.borrow_mut() = Rc::downgrade(&app);
        let weak = Rc::downgrade(&app);
        app._app_monitor.connect_changed(move |_| {
            if let Some(app) = weak.upgrade() {
                app.apps.reindex();
                app.on_apps_changed();
            }
        });
        if let Some(display) = gdk::Display::default() {
            let weak = Rc::downgrade(&app);
            display.monitors().connect_items_changed(move |_, _, _, _| {
                if let Some(app) = weak.upgrade() {
                    app.watch_monitors();
                    app.bars_dirty.set(true);
                    app.schedule_refresh();
                }
            });
        }
        app.watch_monitors();
        app.load_config();
        app.load_pins();
        app.watch_sway();
        app.schedule_refresh();
        app
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code.get()
    }

    fn quit(&self, code: u8) {
        self.exit_code.set(code);
        self.main_loop.quit();
    }

    /// Ao sair: as docks e o swaymsg dos eventos (o setpriv também o encerra, mesmo num SIGKILL).
    pub fn shutdown(&self) {
        for bar in self.bars.borrow_mut().drain(..) {
            bar.destroy();
        }
        if let Err(err) = dl::remove_area(&resident::runtime_dir()) {
            warn(&format!("could not remove the dock area: {err}"));
        }
        if let Some(mut child) = self.watcher.borrow_mut().take() {
            let _ = child.kill();
        }
    }

    /// A área das docks (à vista, ou a faixa da borda), para a grade de apps (sway-dock.area). Depois do que estiver em
    /// andamento: quem avisa pode estar no meio de uma troca das docks, com a lista emprestada.
    fn publish_area(&self) {
        if self.area_queued.replace(true) {
            return;
        }
        let me = self.me.borrow().clone();
        glib::idle_add_local_once(move || {
            let Some(app) = me.upgrade() else { return };
            app.area_queued.set(false);
            let rects: Vec<dl::Rect> = app.bars.borrow().iter().filter_map(|b| b.area()).collect();
            let run = resident::runtime_dir();
            let result = if rects.is_empty() { dl::remove_area(&run) } else { dl::write_area(&run, &rects) };
            if let Err(err) = result {
                warn(&format!("could not publish the dock area: {err}"));
            }
        });
    }

    fn canonical(&self, pins: &[String]) -> Vec<String> {
        let mut seen = Vec::new();
        for pin in pins {
            let key = self.apps.resolve(pin).unwrap_or_else(|| pin.clone());
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        seen
    }

    fn load_config(&self) {
        let (config, warnings) = dl::load_config(&config_path());
        for message in warnings {
            warn(&message);
        }
        let palette = config::palette(&config::load());
        self.provider.load_from_string(&format!(
            "{}{}{CSS}.glyph {{ font-size: {}px; }}\n",
            config::accent_css(&palette),
            config::font_css(system_font_family().as_deref()),
            (config.icon_size as f64 * GLYPH_SCALE) as i64
        ));
        *self.config.borrow_mut() = config;
    }

    pub fn reload(&self) {
        self.load_config();
        self.bars_dirty.set(true);
        if let Some(app) = self.me.borrow().upgrade() {
            app.schedule_refresh();
        }
    }

    /// kill -USR2: o estado de cada dock no log do Sway, para entender uma dock que não esconde.
    pub fn dump(&self) {
        for (index, bar) in self.bars.borrow().iter().enumerate() {
            warn(&format!("dock {index}: {}", bar.describe()));
        }
        let alive = self.retired.borrow().iter().filter(|w| w.strong_count() > 0).count();
        warn(&format!("old docks still alive: {alive} of {} rebuilt", self.retired_total.get()));
    }

    /// O botão esquerdo, pelo que o GDK sabe. Na dúvida, apertado: assim só o tempo sem movimento
    /// encerra o arrasto.
    fn button1_down(&self) -> bool {
        let pointer = gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.pointer());
        match pointer {
            Some(pointer) => pointer.modifier_state().contains(gdk::ModifierType::BUTTON1_MASK),
            None => true,
        }
    }

    fn check_stuck(&self) {
        let down = self.button1_down();
        let bars = self.bars.borrow().clone();
        for bar in bars {
            bar.check_stuck(down);
        }
    }

    fn watch_stuck(self: &Rc<Self>) {
        if self.stuck_timer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local(STUCK_CHECK, move || {
            let Some(app) = weak.upgrade() else { return glib::ControlFlow::Break };
            app.check_stuck();
            let busy = app.bars.borrow().iter().any(|b| b.menu.borrow().is_some() || b.dragging.get());
            if busy {
                return glib::ControlFlow::Continue;
            }
            app.stuck_timer.borrow_mut().take();
            glib::ControlFlow::Break
        });
        *self.stuck_timer.borrow_mut() = Some(timer);
    }

    /// O GDK pode saber da nova geometria depois do evento do Sway: cada monitor avisa a mudança.
    fn watch_monitors(self: &Rc<Self>) {
        let mut watched = self.watched_monitors.borrow_mut();
        let current = all_monitors();
        watched.retain(|m| current.contains(m));
        for monitor in current {
            if watched.contains(&monitor) {
                continue;
            }
            let weak = Rc::downgrade(self);
            monitor.connect_geometry_notify(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.schedule_refresh();
                }
            });
            watched.push(monitor);
        }
    }

    fn on_apps_changed(self: &Rc<Self>) {
        if self.pins_default.get() {
            // Um app instalado agora pode entrar nos padrões; continuam sem gravar
            *self.pins.borrow_mut() = self.default_pins();
            self.publish_pins();
        }
        // Um fixado pode ter passado a ter outro id: sem isso, Desafixar não o encontraria
        let pins = self.canonical(&self.pins.borrow());
        if pins != *self.pins.borrow() && !self.pins_default.get() {
            *self.pins.borrow_mut() = pins;
            self.save_pins();
        }
        for bar in self.bars.borrow().iter() {
            bar.needs_reset.set(true); // só na próxima vez que desenhar: pode haver um menu ou um arrasto
        }
        self.apply();
    }

    // --- Sway ---

    /// Só o aviso importa: cada evento agenda uma leitura da árvore. A leitura das linhas é numa linha
    /// do GIO; a principal só acorda com o evento.
    fn watch_sway(self: &Rc<Self>) {
        // setpriv: o swaymsg morre junto com a dock, mesmo num SIGKILL
        let spawned = Command::new("setpriv")
            .args(["--pdeathsig", "TERM", "swaymsg", "-t", "subscribe", "-m", r#"["window","workspace","output"]"#])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) => {
                warn(&format!("swaymsg: {err}"));
                self.quit(1);
                return;
            }
        };
        let Some(stdout) = child.stdout.take() else { return };
        *self.watcher.borrow_mut() = Some(child);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let mut reader = Some(BufReader::new(stdout));
            loop {
                let Some(mut current) = reader.take() else { break };
                // Bytes, e não texto: um título com UTF-8 inválido (comum no XWayland) chega no evento, e
                // aqui só importa que chegou uma linha
                let result = gio::spawn_blocking(move || {
                    let mut line = Vec::new();
                    let read = current.read_until(b'\n', &mut line);
                    (current, read)
                })
                .await;
                let Ok((current, Ok(read))) = result else { break };
                if read == 0 {
                    break;
                }
                reader = Some(current);
                match weak.upgrade() {
                    Some(app) => app.schedule_refresh(),
                    None => return,
                }
            }
            // O laço do Sway reabre a dock, e o Sway saindo encerra o laço
            if let Some(app) = weak.upgrade() {
                // O swaymsg pode estar vivo (um erro de leitura): encerrado, e o wait fora da linha principal
                if let Some(mut child) = app.watcher.borrow_mut().take() {
                    let _ = child.kill();
                    gio::spawn_blocking(move || child.wait());
                }
                app.quit(1);
            }
        });
    }

    /// Uma rajada de eventos (título mudando, por exemplo) vira uma leitura só.
    fn schedule_refresh(self: &Rc<Self>) {
        if self.refresh_scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(REFRESH, move || {
            if let Some(app) = weak.upgrade() {
                app.start_refresh();
            }
        });
    }

    fn start_refresh(self: &Rc<Self>) {
        self.refresh_scheduled.set(false);
        if self.refresh_running.get() {
            self.refresh_again.set(true);
            return;
        }
        self.refresh_running.set(true);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let tree = gio::spawn_blocking(|| {
                let out = Command::new("timeout").args(["-k", "1", "5", "swaymsg", "-t", "get_tree"]).stdin(Stdio::null()).output().ok()?;
                serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()
            })
            .await
            .ok()
            .flatten();
            if let Some(app) = weak.upgrade() {
                app.apply_tree(tree);
            }
        });
    }

    fn apply_tree(self: &Rc<Self>, tree: Option<serde_json::Value>) {
        self.refresh_running.set(false);
        if let Some(tree) = tree.filter(|t| t.is_object()) {
            *self.windows.borrow_mut() = dl::windows_from_tree(&tree);
            *self.outputs.borrow_mut() = dl::outputs_from_tree(&tree);
            self.apply();
            self.update_polling();
        }
        if self.refresh_again.replace(false) {
            self.schedule_refresh();
        }
    }

    fn update_polling(self: &Rc<Self>) {
        let needed = dl::needs_polling(&self.config.borrow().mode, &self.windows.borrow(), &self.outputs.borrow());
        let running = self.poll_timer.borrow().is_some();
        if needed && !running {
            let weak = Rc::downgrade(self);
            let timer = glib::timeout_add_local(POLL, move || match weak.upgrade() {
                Some(app) => {
                    app.schedule_refresh();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            *self.poll_timer.borrow_mut() = Some(timer);
        } else if !needed && running {
            if let Some(timer) = self.poll_timer.borrow_mut().take() {
                timer.remove();
            }
        }
    }

    // --- tela ---

    /// Pelo nome no Sway, que pode só aparecer depois: quem chama confere de novo a cada leitura.
    fn wanted_monitors(&self) -> Vec<gdk::Monitor> {
        let config = self.config.borrow();
        let outputs = self.outputs.borrow();
        all_monitors()
            .into_iter()
            .filter(|m| {
                config.outputs.is_empty()
                    || dl::output_at(&outputs, monitor_rect(m)).is_some_and(|name| config.outputs.contains(&name))
            })
            .collect()
    }

    fn sync_bars(self: &Rc<Self>) {
        let monitors = self.wanted_monitors();
        let current: Vec<gdk::Monitor> = self.bars.borrow().iter().map(|b| b.monitor.clone()).collect();
        if !self.bars_dirty.get() && monitors == current {
            return;
        }
        self.bars_dirty.set(false);
        let old: Vec<Rc<Bar>> = self.bars.borrow_mut().drain(..).collect();
        let visible: Vec<gdk::Monitor> = old
            .iter()
            .filter(|b| b.window.is_visible() && b.slide.borrow().target == 0.0)
            .map(|b| b.monitor.clone())
            .collect();
        let mut retired = self.retired.borrow_mut();
        retired.retain(|w| w.strong_count() > 0);
        for bar in &old {
            bar.destroy();
            retired.push(Rc::downgrade(bar));
            self.retired_total.set(self.retired_total.get() + 1);
        }
        drop(retired);
        drop(old);
        self.publish_area(); // as docks novas avisam ao aparecer; as que saíram, só aqui
        let bars = monitors.iter().map(|m| Bar::new(self, m, visible.contains(m))).collect();
        *self.bars.borrow_mut() = bars;
    }

    fn apply(self: &Rc<Self>) {
        self.sync_bars();
        self.check_stuck();
        let items = {
            let config = self.config.borrow();
            dl::build_items(&self.pins.borrow(), &self.windows.borrow(), &|id| self.apps.resolve(id), &config.ignore)
        };
        let bars = self.bars.borrow().clone();
        let windows = self.windows.borrow().clone();
        let outputs = self.outputs.borrow().clone();
        for bar in bars {
            bar.set_items(items.clone());
            bar.set_overlap(&windows, &outputs);
        }
    }

    // --- fixados ---

    fn load_pins(&self) {
        let path = dl::pins_path();
        self.pins_default.set(false);
        match dl::read_pins(&path) {
            Ok(Some(pins)) => {
                *self.pins.borrow_mut() = self.canonical(&pins);
                self.pins_unreadable.set(false);
            }
            Ok(None) => {
                *self.pins.borrow_mut() = self.default_pins();
                self.pins_unreadable.set(false);
                self.pins_default.set(true);
            }
            Err(err) => {
                self.pins.borrow_mut().clear();
                self.pins_unreadable.set(true);
                warn(&format!("could not read the pinned apps in {}: {err}", path.display()));
            }
        }
        self.publish_pins();
    }

    /// Os fixados de quem não tem o arquivo (a dock sem a sessão, que copiaria o do skel)
    fn default_pins(&self) -> Vec<String> {
        dl::default_pins(&swaydesk_common::config::load(), &|name| self.apps.resolve(name))
    }

    /// Só depois de uma mudança do usuário: a partir daí, os fixados são dele e vão para o arquivo
    fn save_pins(&self) {
        self.pins_default.set(false);
        let path = dl::pins_path();
        if self.pins_unreadable.get() {
            // Salvar por cima perderia o que ainda pode estar no arquivo
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            match dl::set_aside(&path, now) {
                Ok(target) => warn(&format!("unreadable pinned apps set aside in {}", target.display())),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    warn(&format!("pinned apps not saved: could not set the unreadable file aside: {err}"));
                    self.publish_pins();
                    return;
                }
            }
            self.pins_unreadable.set(false);
        }
        if let Err(err) = dl::save_pins(&path, &self.pins.borrow()) {
            warn(&format!("could not save the pinned apps: {err}"));
        }
        self.publish_pins(); // o que a dock mostra, tenha gravado ou não
    }

    /// O arquivo que a grade de apps lê para o rótulo do menu: os fixados como a dock os mostra. Com o
    /// arquivo dos fixados ilegível, nada é publicado, e a grade não oferece a opção: fixar ali poria o
    /// arquivo ruim de lado e deixaria só aquele app.
    fn publish_pins(&self) {
        let run = resident::runtime_dir();
        let result = if self.pins_unreadable.get() {
            dl::remove_pinned_ids(&run)
        } else {
            // De novo pelo resolve: um app fixado antes de o .desktop dele ser indexado
            let ids: Vec<String> =
                self.pins.borrow().iter().map(|p| self.apps.resolve(p).unwrap_or_else(|| p.clone())).collect();
            dl::write_pinned_ids(&run, &ids)
        };
        if let Err(err) = result {
            warn(&format!("could not publish the pinned apps: {err}"));
            let _ = dl::remove_pinned_ids(&run); // uma lista velha mostraria a opção errada; sem ela, nenhuma
        }
    }

    /// SIGHUP: os pedidos de sway-dock pin|unpin, comparando os apps como a dock compara.
    pub fn pin_requests(&self) {
        let path = requests_path();
        let requests = match dl::take_requests(&path) {
            Ok(requests) => requests,
            Err(err) => {
                warn(&format!("could not read the pin requests in {}: {err}", path.display()));
                let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                if let Ok(target) = dl::set_aside(&path, now) {
                    warn(&format!("pin requests set aside in {}", target.display()));
                }
                return;
            }
        };
        let mut pins = self.pins.borrow().clone();
        for (action, key) in &requests {
            pins = dl::apply_pin_request(&pins, action, key, &|id| self.apps.resolve(id));
        }
        if pins != *self.pins.borrow() {
            *self.pins.borrow_mut() = pins;
            self.save_pins();
            if let Some(app) = self.me.borrow().upgrade() {
                app.apply();
            }
        } else if !requests.is_empty() {
            self.publish_pins(); // nada mudou, mas a grade espera a lista nova para mostrar a opção
        }
    }

    fn toggle_pin(self: &Rc<Self>, key: &str) {
        let pins = dl::toggle_pin(&self.pins.borrow(), key);
        *self.pins.borrow_mut() = pins;
        self.save_pins();
        self.apply();
    }

    fn set_pin_order(self: &Rc<Self>, order: Vec<String>) {
        // Os que estão no ignore ficam
        let hidden: Vec<String> = self.pins.borrow().iter().filter(|p| !order.contains(p)).cloned().collect();
        *self.pins.borrow_mut() = order.into_iter().chain(hidden).collect();
        self.save_pins();
        self.apply();
    }
}
