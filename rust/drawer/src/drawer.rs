//! A janela da grade: GTK 4 e layer-shell. A lógica (busca, contas, fixados, arquivos) fica no crate
//! `swaydesk-drawer-logic`; aqui só a interface, no mesmo comportamento da versão em Python.
//!
//! Cuidado com o RefCell do estado: o GTK chama o filtro e a ordem da grade, e emite sinais, de
//! dentro das próprias chamadas (inserir um item, trocar o texto da busca, mudar o foco). Nenhum
//! método segura um `borrow_mut` enquanto chama o GTK.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, graphene, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::config::{self, Config};
use swaydesk_common::i18n::{ntr, ptr, tr};
use swaydesk_common::{app_scope, paths, renderer, resident};
use swaydesk_drawer_logic as logic;
use swaydesk_drawer_logic::groups::{self, Group};
use swaydesk_drawer_logic::{dock, App, AppFields};

use crate::{warn, PROGRAM};

const ICON_SIZE: i32 = 56;
const FILE_ICON_SIZE: i32 = 20;
const COLUMNS: u32 = 7;
const ITEM_WIDTH: i32 = 116;
const SEARCH_WIDTH: i32 = 480;
const GROUP_ICON_SIZE: i32 = 20;
// O painel, no centro da tela: uma fração dela, com um máximo. Com 75% da altura, ele não chega à Waybar
const PANEL_WIDTH: f64 = 0.70;
const PANEL_MAX_WIDTH: i32 = 1100;
const PANEL_HEIGHT: f64 = 0.75;
const PANEL_MAX_HEIGHT: i32 = 760;
// Depois de um pedido, a opção da dock some até a dock publicar a lista nova
const DOCK_WAIT: Duration = Duration::from_secs(2);
// Depois de filtrar, o tempo para a grade receber as posições novas (um quadro, com folga)
const LAYOUT_WAIT: Duration = Duration::from_millis(100);
const FILE_DELAY: Duration = Duration::from_millis(300);
/// O qalc, para conversões e contas com unidades: depois de uma pausa na digitação, com um prazo
const QALC_DELAY: Duration = Duration::from_millis(150);
const QALC_TIMEOUT: Duration = Duration::from_secs(2);
const MENU_CLICK_GRACE: Duration = Duration::from_millis(300);
const FALLBACK_ICON: &str = "application-x-executable";

/// A paleta é a da Waybar e dos popups. O rodapé (.status) tem fundo opaco. A janela cobre a tela e a
/// escurece como o menu de sessão: as janelas de trás não recebem o teclado nem o mouse, e um clique
/// fora do painel fecha a grade.
const CSS: &str = r#"
window#sway-drawer { background-color: rgba(0, 0, 0, 0.45); color: #dadada; }
#panel {
  background-color: #2e2e33; border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px; padding: 20px 24px 12px 24px; box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5);
}
#panel separator { background-color: rgba(255, 255, 255, 0.08); min-height: 1px; }
/* Mais apps embaixo: uma sombra na borda da área que rola, que some no fim */
#panel > scrolledwindow undershoot.bottom {
  background-image: linear-gradient(to top, rgba(218, 218, 218, 0.14), rgba(218, 218, 218, 0) 12px);
}
label { color: #dadada; }
entry {
  background-color: #38383d; color: #dadada; caret-color: #dadada;
  border: 1px solid rgba(255, 255, 255, 0.06); border-radius: 8px;
  padding: 6px 10px; min-height: 16px; box-shadow: none; outline: none; font-size: 15px;
}
entry:focus-within { border-color: @accent; }
entry image { color: #8a8a8f; }
entry text selection, entry text selection:focus-within { background-color: @accent; color: @on_accent; }
button {
  background: none; border: none; box-shadow: none; text-shadow: none; color: #dadada;
  border-radius: 8px; padding: 4px; outline: none;
}
button:hover { background-color: #4c4c50; }
button:active { background-color: @accent; color: @on_accent; }
flowbox, flowboxchild { background: none; }
flowboxchild { border-radius: 8px; padding: 0; outline: none; }
flowboxchild:focus, flowboxchild:selected, flowboxchild.default { background-color: alpha(@accent, 0.35); }
/* O GTK 4 mede o texto um pouco mais largo que o GTK 3: com 4px dos lados, "KDE Connect SMS" quebrava */
button.app { padding: 4px 1px; }
button.group { margin: 0 3px; padding: 6px 12px; min-width: 72px; }
button.group label { font-size: 12px; }
button.group.current { background-color: @accent; color: @on_accent; }
button.group.current label, button.group.current image { color: @on_accent; }
.app-name { font-size: 13px; }
#pinned { padding-bottom: 10px; border-bottom: 1px solid rgba(255, 255, 255, 0.08); }
#files {
  padding: 6px; background-color: #38383d;
  border: 1px solid rgba(255, 255, 255, 0.06); border-radius: 8px;
}
button.file { padding: 4px 8px; }
button.file:focus, button.file:focus-visible { background-color: alpha(@accent, 0.35); }
button.calc label { font-weight: bold; font-size: 18px; color: @accent; }
.status { color: #8a8a8f; font-size: 12px; background-color: #2e2e33; }
.empty { color: #8a8a8f; font-size: 14px; }
scrolledwindow undershoot.top, scrolledwindow undershoot.bottom,
scrolledwindow overshoot.top, scrolledwindow overshoot.bottom { background: none; box-shadow: none; }
scrollbar, scrollbar trough { background: none; border: none; }
scrollbar slider { background-color: #4c4c50; border-radius: 8px; min-width: 6px; border: none; }
scrollbar slider:hover { background-color: #8a8a8f; }
popover.menu > contents {
  background: #2e2e33; color: #dadada; border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px; padding: 4px; box-shadow: none;
}
popover.menu button, popover.menu checkbutton { padding: 5px 10px; border-radius: 6px; font-size: 13px; }
popover.menu button:hover, popover.menu checkbutton:hover { background: #4c4c50; color: #ffffff; }
popover.menu .section { color: #8a8a8f; font-size: 12px; padding: 4px 10px 0 10px; }
popover.name > contents {
  background: #2e2e33; color: #dadada; border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px; padding: 8px; box-shadow: none;
}
"#;

/// Os nomes dos grupos padrão (as chaves de `groups::DEFAULT_GROUPS`), na língua da sessão enquanto o
/// grupo não for renomeado.
fn group_label(text: &str) -> String {
    match text {
        "Office" => tr("Office"),
        "System" => tr("System"),
        "Utilities" => tr("Utilities"),
        other => other.to_string(),
    }
}

/// Sem esperar, numa sessão própria (`setsid -f`, como o `spawn_detached` do Python): o app aberto pela
/// grade não conta como auxiliar dela no popup "Sistema". O GLib recolhe o processo.
fn run_detached<S: AsRef<OsStr>>(argv: &[S]) -> bool {
    run_program(argv, None)
}

/// Um app (o de terminal, pelo id do .desktop): com a grade num serviço do usuário, ele vai para um escopo
/// próprio (swaydesk_common::app_scope). Para isso o PID tem de ser o do app: o `setsid` sem o `-f`, que troca
/// de sessão e executa o programa no mesmo processo.
fn run_app<S: AsRef<OsStr>>(argv: &[S], app_id: &str) -> bool {
    run_program(argv, Some(app_id))
}

fn run_program<S: AsRef<OsStr>>(argv: &[S], app_id: Option<&str>) -> bool {
    let mut argv: Vec<&OsStr> = argv.iter().map(AsRef::as_ref).collect();
    let scoped = app_id.is_some() && app_scope::wants_scope();
    // O setsid -f sempre "dá certo", mesmo sem o programa: confere antes, como o Python
    let program = Path::new(argv[0]);
    let exists = if program.components().count() > 1 {
        program.is_file() && program.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    } else {
        glib::find_program_in_path(program).is_some()
    };
    if !exists {
        warn(&format!("could not run {}: not found", program.display()));
        return false;
    }
    // Num serviço, o app espera no sh do GATE_SCRIPT até a mudança de escopo (o mesmo PID, sem corrida); o
    // setsid sem o -f troca de sessão no mesmo processo
    let gate = if scoped { app_scope::gate().ok() } else { None };
    if gate.is_some() {
        argv.splice(0..0, app_scope::GATE_SCRIPT.iter().map(OsStr::new));
    }
    if glib::find_program_in_path("setsid").is_some() {
        if gate.is_some() {
            argv.insert(0, OsStr::new("setsid"));
        } else {
            argv.splice(0..0, [OsStr::new("setsid"), OsStr::new("-f")]);
        }
    }
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE);
    renderer::clean_subprocess_launcher(&launcher); // um app GTK 4 aberto pelo script desenharia por CPU
    if scoped {
        for name in app_scope::UNIT_ENV {
            launcher.unsetenv(name);
        }
    }
    let (read, release) = gate.map_or((None, None), |(r, w)| (Some(r), Some(w)));
    if let Some(read) = read {
        launcher.take_fd(read, 3);
    }
    match launcher.spawn(&argv) {
        Ok(child) => {
            let pid = child.identifier().and_then(|p| p.parse::<u32>().ok());
            match (release, app_id, pid) {
                (Some(release), Some(id), Some(pid)) => app_scope::move_then_release(pid, id, release),
                (None, Some(id), Some(pid)) if scoped => app_scope::move_to_scope(pid, id),
                _ => {} // o release, se houver, fecha aqui: o app segue
            }
            true
        }
        Err(err) => {
            warn(&format!("could not run {}: {}", argv[0].to_string_lossy(), err.message()));
            false
        }
    }
}

/// Se uma entrada que o should_show() esconde entra só na busca (os painéis da Central): a decisão é do
/// `logic::search_only`, com o PATH do processo.
fn search_only(info: &gio::DesktopAppInfo) -> bool {
    let try_exec = info.string("TryExec").map(|t| t.to_string());
    let exec = info.executable().to_string_lossy().into_owned();
    let entry = logic::HiddenEntry {
        no_display: info.is_nodisplay(),
        hidden: info.is_hidden(),
        shown_here: info.shows_in(None),
        searchable: info.boolean(logic::SEARCHABLE_KEY),
        try_exec: try_exec.as_deref(),
        exec: Some(&exec),
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    logic::search_only(&entry, |program| logic::program_exists(program, &path))
}

fn app_fields(info: &gio::DesktopAppInfo) -> Option<AppFields> {
    let id = info.id()?.to_string();
    let id = id.strip_suffix(".desktop").unwrap_or(&id).to_string();
    let name = [info.display_name().to_string(), info.name().to_string()]
        .into_iter()
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| id.clone());
    Some(AppFields {
        name,
        generic: info.generic_name().map(|g| g.to_string()).unwrap_or_default(),
        comment: info.description().map(|d| d.to_string()).unwrap_or_default(),
        keywords: info.keywords().iter().map(|k| k.to_string()).collect(),
        executable: info.executable().to_string_lossy().into_owned(),
        categories: logic::parse_categories(info.categories().as_deref().unwrap_or("")),
        terminal: info.boolean("Terminal"),
        search_only: false,
        id,
    })
}

/// O ícone do .desktop no tamanho pedido; sem ele, um genérico. Um ícone por caminho tenta o próprio
/// caminho e as extensões (`dock::icon_paths`), e o que não abrir cai no genérico.
fn icon_image(icon: Option<&gio::Icon>, size: i32) -> gtk4::Image {
    let image = match icon {
        Some(icon) => {
            if let Some(file_icon) = icon.downcast_ref::<gio::FileIcon>() {
                file_icon.file().path().and_then(|path| {
                    dock::icon_paths(&path.to_string_lossy())
                        .into_iter()
                        .filter(|candidate| Path::new(candidate).is_file())
                        .find_map(|candidate| gdk::Texture::from_filename(&candidate).ok())
                        .map(|texture| gtk4::Image::from_paintable(Some(&texture)))
                })
            } else {
                let display = gdk::Display::default();
                let known = display.map(|d| gtk4::IconTheme::for_display(&d).has_gicon(icon)).unwrap_or(false);
                known.then(|| gtk4::Image::from_gicon(icon))
            }
        }
        None => None,
    };
    let image = image.unwrap_or_else(|| gtk4::Image::from_icon_name(FALLBACK_ICON));
    image.set_pixel_size(size);
    image
}

fn label(text: &str, class: Option<&str>) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    if let Some(class) = class {
        label.add_css_class(class);
    }
    label
}

fn system_font_family() -> Option<String> {
    let name = gtk4::Settings::default()?.gtk_font_name()?;
    pango::FontDescription::from_string(&name).family().map(|f| f.to_string())
}

fn remove_children(container: &impl IsA<gtk4::Widget>, remove: impl Fn(&gtk4::Widget)) {
    while let Some(child) = container.as_ref().first_child() {
        remove(&child);
    }
}

struct Widgets {
    window: gtk4::Window,
    /// O painel no centro: um clique fora dele fecha a grade
    panel: gtk4::Box,
    entry: gtk4::SearchEntry,
    groups_box: gtk4::Box,
    calc_button: gtk4::Button,
    scroller: gtk4::ScrolledWindow,
    pinned: gtk4::FlowBox,
    grid: gtk4::FlowBox,
    empty: gtk4::Label,
    files: gtk4::Box,
    status: gtk4::Label,
    /// Um só menu por tipo, reaproveitado: o residente não junta um a cada clique direito
    menu: gtk4::Popover,
    menu_button: gtk4::Button,
    dock_button: gtk4::Button,
    groups_title: gtk4::Label,
    groups_checks: gtk4::Box,
    remove_button: gtk4::Button,
    group_menu: gtk4::Popover,
    rename_button: gtk4::Button,
    delete_button: gtk4::Button,
    /// O nome de um grupo novo ou renomeado: um só popover, reaproveitado
    name_popover: gtk4::Popover,
    name_entry: gtk4::Entry,
}

/// O que o campo do popover faz.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NameTarget {
    Add,
    Rename(usize),
}

#[derive(Default)]
struct State {
    /// Por nome
    apps: Vec<App>,
    index: HashMap<String, usize>,
    infos: HashMap<String, gio::DesktopAppInfo>,
    /// Os itens da grade e dos fixados, com o índice do app
    children: HashMap<gtk4::FlowBoxChild, usize>,
    /// O item de cada app na grade principal
    child_of: HashMap<String, gtk4::FlowBoxChild>,
    /// A posição de cada app visível
    order: HashMap<String, usize>,
    ranked: Vec<usize>,
    groups: Vec<Group>,
    groups_readable: bool,
    /// O índice em `groups`; None é o Início
    group: Option<usize>,
    group_buttons: Vec<(Option<usize>, gtk4::Button)>,
    add_button: Option<gtk4::Button>,
    name_target: Option<NameTarget>,
    group_menu_index: Option<usize>,
    panel_size: Option<(i32, i32)>,
    /// O app de onde o teclado desceu para os arquivos
    grid_return: Option<gtk4::FlowBoxChild>,
    /// id -> (se pediu para fixar, prazo): pedido feito, a dock ainda não publicou o estado pedido
    dock_waiting: HashMap<String, (bool, Instant)>,
    dock_action: Option<&'static str>,
    /// Quando o filtro mudou: até o próximo layout, as posições dos itens ainda são as antigas
    filtered_at: Option<Instant>,
    apps_dirty: bool,
    file_timer: Option<glib::SourceId>,
    calc_text: Option<String>,
    /// Só a consulta mais recente ao qalc vale; as atrasadas são descartadas
    qalc_generation: u64,
    qalc_timer: Option<glib::SourceId>,
    qalc_pending: bool,
    /// O ajuste das linhas inteiras já marcado para depois do desenho
    rows_queued: bool,
    /// O qalc em curso: um texto novo o encerra, e fica só um por vez
    qalc_proc: Option<gio::Subprocess>,
    /// O app que o Enter na busca abre, marcado na grade
    default_child: Option<gtk4::FlowBoxChild>,
    /// O app sob o mouse, enquanto o teclado não escolher outro
    hovered: Option<usize>,
    background_pressed: bool,
    /// Quando o menu de Fixar fechou: o clique fora dele, que o fecha, pode chegar também à janela
    menu_closed: Option<Instant>,
    pins: Vec<String>,
    pins_readable: bool,
    menu_app: Option<String>,
}

pub struct Drawer {
    /// Para os métodos de &self que precisam criar ligações (o hide refaz a lista)
    me: Weak<Drawer>,
    w: Widgets,
    s: RefCell<State>,
    /// Muda a cada busca e ao esconder: cancela a busca de arquivos em andamento, em outra linha
    file_generation: Arc<AtomicU64>,
    pins_path: PathBuf,
    groups_path: PathBuf,
    _monitor: gio::AppInfoMonitor,
    /// O arquivo da área da dock: muda com a dock subindo ou descendo, com a grade aberta
    dock_area: Option<gio::FileMonitor>,
}

impl Drawer {
    pub fn new(config: &Config) -> Rc<Drawer> {
        swaydesk_ui::always_dark();
        if let Some(settings) = gtk4::Settings::default() {
            // O cursor piscando na busca redesenha a janela inteira a cada piscada: com a grade
            // aberta e parada, 2 a 3% de um núcleo no GL, e cada abertura custava 30 ms em vez de 17
            // (medido nas VMs). Só neste processo: o resto do desktop segue o gsettings
            settings.set_gtk_cursor_blink(false);
        }
        let palette = config::palette(config);
        let css = format!("{}{}{CSS}", config::accent_css(&palette), config::font_css(system_font_family().as_deref()));
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&css);
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }

        let pins_path = config::data_home().join(PROGRAM).join("pinned");
        let legacy = config::cache_home().join("nwg-pin-cache"); // os fixados do nwg-drawer
        let (pins, pins_readable) = match logic::load_pins(&pins_path, Some(&legacy)) {
            Ok(pins) => (pins, true),
            Err(err) => {
                warn(&format!("could not read the pinned apps: {err}"));
                (vec![], false)
            }
        };
        let groups_path = config::data_home().join(PROGRAM).join("groups.json");
        let (groups, groups_readable) = match groups::load_groups(&groups_path) {
            Ok(groups) => (groups, true),
            Err(err) => {
                warn(&format!("could not read the groups: {err}"));
                (vec![], false)
            }
        };
        let drawer = Rc::new_cyclic(|me| Drawer {
            me: me.clone(),
            w: make_widgets(),
            s: RefCell::new(State { pins, pins_readable, groups, groups_readable, ..State::default() }),
            file_generation: Arc::new(AtomicU64::new(0)),
            pins_path,
            groups_path,
            _monitor: gio::AppInfoMonitor::get(),
            dock_area: watch_dock_area(),
        });
        drawer.connect();
        drawer.load_apps();
        drawer
    }

    fn connect(self: &Rc<Self>) {
        let w = &self.w;

        // Só linhas inteiras à vista: quando a área ou o conteúdo muda de altura
        let adjustment = w.scroller.vadjustment();
        let d = self.clone();
        adjustment.connect_page_size_notify(move |_| d.queue_whole_rows());
        let d = self.clone();
        adjustment.connect_upper_notify(move |_| d.queue_whole_rows());

        let d = self.clone();
        w.window.connect_close_request(move |_| {
            d.hide();
            glib::Propagation::Stop
        });
        // Com o foco na grade, a marca fica só no item focado. Depois da troca, quando o foco já mudou
        let d = self.clone();
        w.window.connect_focus_widget_notify(move |_| {
            let d = d.clone();
            glib::idle_add_local_once(move || d.keyboard_moved());
        });

        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let d = self.clone();
        keys.connect_key_pressed(move |_, key, _code, state| d.on_key(key, state));
        w.window.add_controller(keys);

        // Esconde na soltura, e não no aperto: escondida com o botão apertado, a janela não recebe a
        // soltura, e o GTK fica achando que o botão continua apertado
        let click = gtk4::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let d = self.clone();
        click.connect_pressed(move |_, _, x, y| {
            let menu_just_closed = d.open_popover().is_some()
                || d.s.borrow().menu_closed.is_some_and(|t| t.elapsed() < MENU_CLICK_GRACE);
            let background = !menu_just_closed && d.on_background(x, y);
            d.s.borrow_mut().background_pressed = background;
        });
        let d = self.clone();
        click.connect_released(move |_, _, x, y| {
            let pressed = std::mem::take(&mut d.s.borrow_mut().background_pressed);
            if pressed && d.on_background(x, y) {
                d.hide();
            }
        });
        w.window.add_controller(click);

        // O "changed", e não o "search-changed", que chega depois: um Enter logo depois de digitar
        // abriria o primeiro app da lista antiga
        let d = self.clone();
        w.entry.connect_changed(move |_| {
            d.s.borrow_mut().hovered = None; // digitar também é escolher pelo teclado
            d.apply_filter();
        });
        let d = self.clone();
        w.entry.connect_activate(move |_| d.on_entry_activate());

        let d = self.clone();
        w.calc_button.connect_clicked(move |_| d.copy_result());

        for menu in [&w.menu, &w.group_menu] {
            let d = self.clone();
            menu.connect_closed(move |_| d.s.borrow_mut().menu_closed = Some(Instant::now()));
        }
        let d = self.clone();
        w.menu_button.connect_clicked(move |_| {
            d.w.menu.popdown();
            let app = d.s.borrow().menu_app.clone();
            if let Some(app) = app {
                d.toggle_pin(&app);
            }
        });
        let d = self.clone();
        w.dock_button.connect_clicked(move |_| {
            d.w.menu.popdown();
            let (app, action) = {
                let s = d.s.borrow();
                (s.menu_app.clone(), s.dock_action)
            };
            if let (Some(app), Some(action)) = (app, action) {
                d.request_dock(action, &app);
            }
        });
        let d = self.clone();
        w.remove_button.connect_clicked(move |_| {
            d.w.menu.popdown();
            let app = d.s.borrow().menu_app.clone();
            if let Some(app) = app {
                d.remove_from_group(&app);
            }
        });
        let d = self.clone();
        w.rename_button.connect_clicked(move |_| {
            d.w.group_menu.popdown();
            let index = d.s.borrow().group_menu_index;
            if let Some(index) = index {
                d.ask_name(NameTarget::Rename(index));
            }
        });
        let d = self.clone();
        w.delete_button.connect_clicked(move |_| {
            d.w.group_menu.popdown();
            let index = d.s.borrow().group_menu_index;
            if let Some(index) = index {
                d.delete_group(index);
            }
        });
        let d = self.clone();
        w.name_entry.connect_activate(move |_| d.apply_name());
        // Fechado por qualquer caminho (Enter, Esc, clique fora): o teclado volta para a busca
        let d = self.clone();
        w.name_popover.connect_closed(move |_| {
            {
                let mut s = d.s.borrow_mut();
                s.name_target = None;
                s.menu_closed = Some(Instant::now()); // o clique que o fechou não fecha a grade
            }
            if d.w.window.is_visible() {
                d.focus_entry();
            }
        });
        // O painel segue o tamanho da tela em que a grade abriu
        let d = self.clone();
        w.window.connect_default_width_notify(move |_| d.fit_to_window());
        let d = self.clone();
        w.window.connect_map(move |_| {
            let d = d.clone();
            glib::idle_add_local_once(move || {
                d.fit_to_window();
                d.leave_dock_free();
            });
        });
        // No Wayland, o monitor da grade só se sabe com o enter da surface, que pode vir depois do map: sem
        // isso, a área da dock de outro monitor viraria um buraco no lugar errado
        let d = Rc::downgrade(self);
        w.window.connect_realize(move |window| {
            let Some(surface) = window.surface() else { return };
            let d = d.clone();
            surface.connect_enter_monitor(move |_, _| {
                if let Some(d) = d.upgrade() {
                    d.leave_dock_free();
                }
            });
        });
        if let Some(watch) = &self.dock_area {
            let d = Rc::downgrade(self);
            watch.connect_changed(move |_, _, _, _| {
                if let Some(d) = d.upgrade() {
                    if d.w.window.is_visible() {
                        d.leave_dock_free();
                    }
                }
            });
        }

        for grid in [&w.pinned, &w.grid] {
            let d = self.clone();
            grid.connect_child_activated(move |_, child| {
                let id = d.app_of(child).map(|i| d.s.borrow().apps[i].id().to_string());
                if let Some(id) = id {
                    d.launch(&id);
                }
            });
            let d = self.clone();
            grid.connect_keynav_failed(move |grid, direction| d.on_keynav_failed(grid, direction));
        }
        let d = self.clone();
        w.grid.set_filter_func(move |child| {
            let s = d.s.borrow();
            s.children.get(child).is_some_and(|&i| s.order.contains_key(s.apps[i].id()))
        });
        let d = self.clone();
        w.grid.set_sort_func(move |a, b| {
            let s = d.s.borrow();
            let position =
                |c: &gtk4::FlowBoxChild| s.children.get(c).and_then(|&i| s.order.get(s.apps[i].id())).copied().unwrap_or(s.order.len());
            position(a).cmp(&position(b)).into()
        });

        // Com a grade aberta, refazer tudo tiraria os itens de baixo do mouse: espera fechar
        let d = self.clone();
        self._monitor.connect_changed(move |_| {
            d.s.borrow_mut().apps_dirty = true;
            if !d.w.window.is_visible() {
                d.load_apps();
            }
        });
    }

    fn app_of(&self, child: &gtk4::FlowBoxChild) -> Option<usize> {
        self.s.borrow().children.get(child).copied()
    }

    // Apps

    fn load_apps(self: &Rc<Self>) {
        let mut infos = HashMap::new();
        let mut apps = Vec::new();
        for info in gio::AppInfo::all() {
            let Ok(info) = info.downcast::<gio::DesktopAppInfo>() else { continue };
            // Os NoDisplay com a X-Swaydesk-Searchable entram só na busca
            let search_only = !info.should_show() && search_only(&info);
            if !info.should_show() && !search_only {
                continue;
            }
            if let Some(fields) = app_fields(&info).map(|f| AppFields { search_only, ..f }) {
                infos.insert(fields.id.clone(), info);
                apps.push(App::new(fields));
            }
        }
        logic::sort_apps(&mut apps);
        let old: Vec<gtk4::FlowBoxChild> = {
            let mut s = self.s.borrow_mut();
            s.index = apps.iter().enumerate().map(|(i, a)| (a.id().to_string(), i)).collect();
            s.apps = apps;
            s.infos = infos;
            s.order.clear();
            s.ranked.clear();
            s.default_child = None; // o antigo sai com a grade
            s.hovered = None;
            s.child_of.clear();
            s.children.drain().map(|(child, _)| child).collect()
        };
        for child in old {
            if let Some(parent) = child.parent().and_downcast::<gtk4::FlowBox>() {
                parent.remove(&child);
            }
        }
        let count = self.s.borrow().apps.len();
        for i in 0..count {
            let child = self.make_item(i, &self.w.grid);
            let id = self.s.borrow().apps[i].id().to_string();
            self.s.borrow_mut().child_of.insert(id, child);
        }
        self.build_pinned();
        self.build_groups();
        self.s.borrow_mut().apps_dirty = false;
        self.apply_filter();
    }

    /// Um app: ícone e nome. O botão não pega o foco: as setas andam pelos itens da grade.
    fn make_item(self: &Rc<Self>, index: usize, grid: &gtk4::FlowBox) -> gtk4::FlowBoxChild {
        let (id, name) = {
            let s = self.s.borrow();
            let app = &s.apps[index];
            (app.id().to_string(), app.fields.name.clone())
        };
        let icon = self.s.borrow().infos.get(&id).and_then(|info| info.icon());
        let button = gtk4::Button::new();
        button.add_css_class("app");
        button.set_focusable(false);
        button.set_focus_on_click(false);
        button.set_size_request(ITEM_WIDTH, -1);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        column.append(&icon_image(icon.as_ref(), ICON_SIZE));
        let text = label(&name, Some("app-name"));
        text.set_justify(gtk4::Justification::Center);
        text.set_wrap(true);
        text.set_wrap_mode(pango::WrapMode::WordChar);
        text.set_lines(2);
        text.set_ellipsize(pango::EllipsizeMode::End);
        text.set_max_width_chars(12);
        column.append(&text);
        button.set_child(Some(&column));

        let d = self.clone();
        let launch_id = id.clone();
        button.connect_clicked(move |_| d.launch(&launch_id));
        let motion = gtk4::EventControllerMotion::new();
        let d = self.clone();
        motion.connect_enter(move |_, _, _| d.set_hovered(Some(index)));
        let d = self.clone();
        motion.connect_leave(move |_| d.set_hovered(None));
        button.add_controller(motion);
        let right = gtk4::GestureClick::new();
        right.set_button(gdk::BUTTON_SECONDARY);
        let d = self.clone();
        let menu_id = id;
        right.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if let Some(widget) = gesture.widget() {
                d.open_menu(&menu_id, &widget, x, y);
            }
        });
        button.add_controller(right);

        let child = gtk4::FlowBoxChild::new();
        child.set_child(Some(&button));
        grid.insert(&child, -1);
        self.s.borrow_mut().children.insert(child.clone(), index);
        child
    }

    fn build_pinned(self: &Rc<Self>) {
        let old: Vec<gtk4::FlowBoxChild> = {
            let s = self.s.borrow();
            s.children.keys().filter(|c| c.parent().as_ref() == Some(self.w.pinned.upcast_ref())).cloned().collect()
        };
        for child in old {
            self.s.borrow_mut().children.remove(&child);
            self.w.pinned.remove(&child);
        }
        let pinned: Vec<usize> = {
            let s = self.s.borrow();
            s.pins.iter().filter_map(|pin| s.index.get(pin).copied()).filter(|&i| s.apps[i].pinnable()).collect()
        };
        for index in pinned {
            self.make_item(index, &self.w.pinned);
        }
    }

    // Grupos

    /// Um botão da barra de baixo: o ícone em cima e o nome embaixo.
    fn group_button(text: &str, icon: &str) -> gtk4::Button {
        let button = gtk4::Button::new();
        button.add_css_class("group");
        button.set_focusable(false);
        button.set_focus_on_click(false);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let image = gtk4::Image::from_icon_name(icon);
        image.set_pixel_size(GROUP_ICON_SIZE);
        column.append(&image);
        let name = label(text, Some("group-name"));
        name.set_ellipsize(pango::EllipsizeMode::End);
        name.set_max_width_chars(14);
        column.append(&name);
        button.set_child(Some(&column));
        button
    }

    fn build_groups(self: &Rc<Self>) {
        remove_children(&self.w.groups_box, |c| self.w.groups_box.remove(c));
        let (names, readable) = {
            let s = self.s.borrow();
            let names: Vec<String> = s.groups.iter().map(|g| groups::display_name(g, &group_label)).collect();
            (names, s.groups_readable)
        };
        let mut buttons = Vec::new();
        let home = Self::group_button(&ptr("app grid", "Home"), "go-home-symbolic");
        let d = self.clone();
        home.connect_clicked(move |_| d.set_group(None));
        self.w.groups_box.append(&home);
        buttons.push((None, home));
        for (index, name) in names.iter().enumerate() {
            let button = Self::group_button(name, "folder-symbolic");
            let d = self.clone();
            button.connect_clicked(move |_| d.set_group(Some(index)));
            let right = gtk4::GestureClick::new();
            right.set_button(gdk::BUTTON_SECONDARY);
            let d = self.clone();
            right.connect_pressed(move |gesture, _, x, y| {
                if !d.s.borrow().groups_readable {
                    return;
                }
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                if let Some(widget) = gesture.widget() {
                    d.s.borrow_mut().group_menu_index = Some(index);
                    d.popup_at(&d.w.group_menu, &widget, x, y);
                }
            });
            button.add_controller(right);
            self.w.groups_box.append(&button);
            buttons.push((Some(index), button));
        }
        let add = Self::group_button(&tr("Add group"), "list-add-symbolic");
        let d = self.clone();
        add.connect_clicked(move |_| d.ask_name(NameTarget::Add));
        // Com o arquivo ilegível, nada pode ser gravado: sem os botões que só gravariam
        add.set_visible(readable);
        self.w.groups_box.append(&add);
        {
            let mut s = self.s.borrow_mut();
            if !buttons.iter().any(|(key, _)| *key == s.group) {
                s.group = None;
            }
            s.group_buttons = buttons;
            s.add_button = Some(add);
        }
        self.mark_group();
    }

    fn mark_group(&self) {
        // A busca procura em todos os apps: enquanto houver busca, o marcado é o Início
        let current = if self.query().is_empty() { self.s.borrow().group } else { None };
        let buttons = self.s.borrow().group_buttons.clone();
        for (key, button) in buttons {
            if key == current {
                button.add_css_class("current");
            } else {
                button.remove_css_class("current");
            }
        }
    }

    fn focus_in_grid(&self) -> bool {
        GtkWindowExt::focus(&self.w.window)
            .and_then(|f| f.downcast::<gtk4::FlowBoxChild>().ok())
            .is_some_and(|c| self.s.borrow().children.contains_key(&c))
    }

    fn set_group(self: &Rc<Self>, key: Option<usize>) {
        let in_grid = self.focus_in_grid(); // Ctrl+PageDown com um app escolhido
        self.s.borrow_mut().group = key;
        self.mark_group();
        self.w.scroller.vadjustment().set_value(0.0);
        if self.w.entry.text().is_empty() {
            self.apply_filter();
        } else {
            self.w.entry.set_text(""); // o changed refaz o filtro
        }
        // O app escolhido pode ter saído com o filtro: o teclado passa ao primeiro do grupo novo
        if in_grid && !(self.focus_pinned() || self.focus_first()) {
            self.focus_entry();
        }
    }

    fn cycle_group(self: &Rc<Self>, step: i32) {
        let (count, current) = {
            let s = self.s.borrow();
            (s.groups.len() as i32 + 1, if self.query().is_empty() { s.group } else { None })
        };
        // A posição 0 é o Início; a n é o grupo n - 1
        let position = current.map_or(0, |i| i as i32 + 1);
        let next = (position + step).rem_euclid(count);
        self.set_group(if next == 0 { None } else { Some(next as usize - 1) });
    }

    fn groups_writable(&self) -> bool {
        let readable = self.s.borrow().groups_readable;
        if !readable {
            warn("the groups could not be read: not saving over them");
        }
        readable
    }

    /// O popover com o nome: um grupo novo, ou o índice do grupo a renomear.
    fn ask_name(self: &Rc<Self>, target: NameTarget) {
        if !self.groups_writable() {
            return;
        }
        let (text, anchor) = {
            let s = self.s.borrow();
            match target {
                NameTarget::Add => (groups::unique_name(&s.groups, &tr("Group"), None, &group_label), s.add_button.clone()),
                NameTarget::Rename(index) => {
                    let Some(group) = s.groups.get(index) else { return };
                    let button = s.group_buttons.iter().find(|(k, _)| *k == Some(index)).map(|(_, b)| b.clone());
                    (groups::display_name(group, &group_label), button)
                }
            }
        };
        let Some(anchor) = anchor else { return };
        self.s.borrow_mut().name_target = Some(target);
        self.w.name_entry.set_text(&text);
        if let Some(bounds) = anchor.compute_bounds(&self.w.window) {
            let rect = gdk::Rectangle::new(bounds.x() as i32, bounds.y() as i32, bounds.width() as i32, bounds.height() as i32);
            self.w.name_popover.set_pointing_to(Some(&rect));
        }
        self.w.name_popover.popup();
        self.w.name_entry.grab_focus();
        self.w.name_entry.select_region(0, -1); // digitar troca o nome sugerido
    }

    fn apply_name(self: &Rc<Self>) {
        let name = groups::py_strip(&self.w.name_entry.text()).to_string();
        let target = self.s.borrow_mut().name_target.take();
        self.w.name_popover.popdown(); // o closed devolve o foco à busca
        let Some(target) = target else { return };
        if name.is_empty() {
            return;
        }
        let groups = self.s.borrow().groups.clone();
        match target {
            NameTarget::Add => {
                self.save_groups(groups::add_group(&groups, &name, &group_label));
                let last = self.s.borrow().groups.len().checked_sub(1);
                self.set_group(last);
            }
            NameTarget::Rename(index) => {
                // O mesmo nome, mesmo com outras maiúsculas ou acentos: o grupo padrão segue traduzido
                let Some(group) = groups.get(index) else { return };
                if logic::normalize(&name) != logic::normalize(&groups::display_name(group, &group_label)) {
                    self.save_groups(groups::rename_group(&groups, index, &name, &group_label));
                }
            }
        }
    }

    fn delete_group(self: &Rc<Self>, index: usize) {
        if !self.groups_writable() {
            return;
        }
        let groups = {
            let mut s = self.s.borrow_mut();
            // O grupo aberto continua aberto: os seguintes ao apagado descem uma posição
            s.group = match s.group {
                Some(open) if open == index => None,
                Some(open) if index < open => Some(open - 1),
                other => other,
            };
            groups::delete_group(&s.groups, index)
        };
        self.save_groups(groups);
    }

    fn save_groups(self: &Rc<Self>, groups: Vec<Group>) {
        if let Err(err) = groups::save_groups(&self.groups_path, &groups) {
            warn(&format!("could not save the groups: {err}"));
        }
        self.s.borrow_mut().groups = groups;
        self.build_groups();
        self.apply_filter();
    }

    fn toggle_in_group(self: &Rc<Self>, index: usize, app_id: &str, inside: bool) {
        if !self.groups_writable() {
            return;
        }
        let groups = {
            let s = self.s.borrow();
            if inside {
                groups::add_app(&s.groups, index, app_id)
            } else {
                let Some(&i) = s.index.get(app_id) else { return };
                groups::remove_app(&s.groups, index, &s.apps[i])
            }
        };
        self.save_groups(groups);
    }

    fn remove_from_group(self: &Rc<Self>, app_id: &str) {
        let group = self.s.borrow().group;
        if let Some(index) = group {
            self.toggle_in_group(index, app_id, false);
        }
    }

    // O painel

    /// A margem que o whole_rows ajusta muda o tamanho da área: fora do size_allocate, depois do desenho
    fn queue_whole_rows(self: &Rc<Self>) {
        if std::mem::replace(&mut self.s.borrow_mut().rows_queued, true) {
            return;
        }
        let d = self.clone();
        glib::idle_add_local_once(move || {
            d.s.borrow_mut().rows_queued = false;
            d.whole_rows();
        });
    }

    /// Com mais apps do que cabem, a área que rola mostra só linhas inteiras, e o que sobra fica embaixo, antes
    /// do rodapé: a última linha à vista não sai com os ícones sem os nomes (logic::row_gap)
    fn whole_rows(&self) {
        let w = &self.w;
        let adjustment = w.scroller.vadjustment();
        let current = w.scroller.margin_bottom() as f64;
        // Onde termina cada item visível dos favoritos e da grade, no conteúdo que rola
        let mut bottoms = Vec::new();
        if !w.files.is_visible() {
            for grid in [&w.pinned, &w.grid] {
                let Some(column) = grid.parent() else { continue };
                if !grid.is_visible() {
                    continue;
                }
                let mut child = grid.first_child();
                while let Some(tile) = child {
                    if tile.is_visible() && tile.is_child_visible() {
                        let end = graphene::Point::new(0.0, tile.height() as f32);
                        if let Some(point) = tile.compute_point(&column, &end) {
                            bottoms.push(point.y() as f64);
                        }
                    }
                    child = tile.next_sibling();
                }
            }
        }
        let gap = logic::row_gap(adjustment.page_size() + current, adjustment.upper(), &bottoms);
        if (gap - current).abs() >= 1.0 {
            w.scroller.set_margin_bottom(gap.round() as i32);
        }
    }

    /// O tamanho do painel numa tela de width x height. Só muda quando a tela muda.
    fn fit_panel(&self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let size = (
            PANEL_MAX_WIDTH.min((width as f64 * PANEL_WIDTH) as i32),
            PANEL_MAX_HEIGHT.min((height as f64 * PANEL_HEIGHT) as i32),
        );
        if self.s.borrow().panel_size != Some(size) {
            self.s.borrow_mut().panel_size = Some(size);
            self.w.panel.set_size_request(size.0, size.1);
        }
    }

    /// Antes de mostrar, pela tela onde a janela esteve ou, na primeira vez, pela primeira tela: o painel
    /// já aparece no tamanho certo. O Sway escolhe a tela do foco; se for outra, o fit_to_window corrige.
    fn fit_panel_before_show(&self) {
        let Some(display) = gdk::Display::default() else { return };
        let monitor = self
            .w
            .window
            .surface()
            .and_then(|surface| display.monitor_at_surface(&surface))
            .or_else(|| display.monitors().item(0).and_downcast::<gdk::Monitor>());
        if let Some(monitor) = monitor {
            let geometry = monitor.geometry();
            self.fit_panel(geometry.width(), geometry.height());
        }
    }

    /// Pelo tamanho que a janela, que cobre a tela, recebeu.
    fn fit_to_window(&self) {
        self.fit_panel(self.w.window.width(), self.w.window.height());
    }

    // Busca

    fn query(&self) -> String {
        self.w.entry.text().trim().to_string()
    }

    fn apply_filter(self: &Rc<Self>) {
        let query = self.query();
        {
            let mut s = self.s.borrow_mut();
            let ranked: Vec<usize> = if !query.is_empty() {
                logic::search(&s.apps, &query).iter().filter_map(|app| s.index.get(app.id()).copied()).collect()
            } else {
                // Sem busca, a grade e os grupos ficam sem os só da busca
                logic::browse(&s.apps, s.group.and_then(|g| s.groups.get(g)))
            };
            s.order = ranked.iter().enumerate().map(|(pos, &i)| (s.apps[i].id().to_string(), pos)).collect();
            s.ranked = ranked;
        }
        self.w.grid.invalidate_filter();
        self.w.grid.invalidate_sort();
        self.s.borrow_mut().filtered_at = Some(Instant::now());
        let all = query.is_empty() && self.s.borrow().group.is_none();
        self.w.pinned.set_visible(all && self.w.pinned.first_child().is_some());
        self.mark_group();
        self.update_calc(&query);
        self.update_empty();
        if !query.is_empty() {
            self.w.scroller.vadjustment().set_value(0.0); // o primeiro, que o Enter abre, à vista
        }
        self.mark_default();
        self.update_status();
        self.schedule_files(&query);
    }

    fn entry_focused(&self) -> bool {
        GtkWindowExt::focus(&self.w.window).is_some_and(|f| f == *self.w.entry.upcast_ref::<gtk4::Widget>() || f.is_ancestor(&self.w.entry))
    }

    /// O app que o Enter na busca abre: o primeiro da lista, só quando há busca.
    fn default_app(&self) -> Option<usize> {
        if self.query().is_empty() {
            return None;
        }
        self.s.borrow().ranked.first().copied()
    }

    /// Marca na grade o app que o Enter abre, enquanto o foco está na busca.
    fn mark_default(&self) {
        let app = if self.entry_focused() { self.default_app() } else { None };
        let (old, new) = {
            let s = self.s.borrow();
            (s.default_child.clone(), app.and_then(|i| s.child_of.get(s.apps[i].id()).cloned()))
        };
        if old == new {
            return;
        }
        if let Some(old) = &old {
            old.remove_css_class("default");
        }
        if let Some(new) = &new {
            new.add_css_class("default");
        }
        self.s.borrow_mut().default_child = new;
    }

    /// O app escolhido pelo teclado: o item focado na grade ou, na busca, o que o Enter abre.
    fn selected_app(&self) -> Option<usize> {
        let focus = GtkWindowExt::focus(&self.w.window)?;
        if let Some(child) = focus.downcast_ref::<gtk4::FlowBoxChild>() {
            return self.app_of(child);
        }
        if self.entry_focused() {
            self.default_app()
        } else {
            None
        }
    }

    /// No rodapé, a descrição do app sob o mouse; sem ele, a do escolhido pelo teclado; sem
    /// nenhum, quantos apps há.
    fn update_status(&self) {
        let hovered = self.s.borrow().hovered;
        let text = match hovered.or_else(|| self.selected_app()) {
            Some(i) => {
                let s = self.s.borrow();
                let f = &s.apps[i].fields;
                let text = [&f.comment, &f.generic, &f.name].into_iter().find(|t| !t.is_empty()).cloned();
                text.unwrap_or_default()
            }
            None => {
                let count = self.s.borrow().ranked.len() as u64;
                ntr("%d application", "%d applications", count)
            }
        };
        self.w.status.set_text(&text);
    }

    fn set_hovered(&self, app: Option<usize>) {
        self.s.borrow_mut().hovered = app;
        self.update_status();
    }

    /// O teclado escolheu outro app: o rodapé passa a ele, mesmo com o mouse parado em cima de
    /// outro, até o mouse entrar em outro app.
    fn keyboard_moved(&self) {
        self.s.borrow_mut().hovered = None;
        self.mark_default();
        self.update_status();
    }

    fn update_empty(&self) {
        let nothing = {
            let s = self.s.borrow();
            s.ranked.is_empty() && s.calc_text.is_none() && !s.qalc_pending
        };
        self.w.empty.set_visible(nothing);
    }

    fn show_calc(&self, text: Option<String>) {
        match &text {
            Some(text) => {
                self.w.calc_button.set_label(&format!("= {text}"));
                self.w.calc_button.set_visible(true);
            }
            None => self.w.calc_button.set_visible(false),
        }
        self.s.borrow_mut().calc_text = text;
    }

    fn update_calc(self: &Rc<Self>, query: &str) {
        let (generation, timer, proc) = {
            let mut s = self.s.borrow_mut();
            s.qalc_generation += 1;
            (s.qalc_generation, s.qalc_timer.take(), s.qalc_proc.take())
        };
        if let Some(timer) = timer {
            timer.remove(); // solto antes da chamada ao GLib
        }
        if let Some(proc) = proc {
            proc.force_exit(); // o resultado dele já não vale
        }
        if let Some(value) = logic::calculate(query) {
            self.s.borrow_mut().qalc_pending = false;
            self.show_calc(Some(logic::format_number(value, &ptr("decimal separator", "."))));
        } else if logic::qalc_candidate(query) && glib::find_program_in_path("qalc").is_some() {
            // O resultado anterior fica até o novo chegar: o botão não pisca a cada tecla
            self.s.borrow_mut().qalc_pending = true;
            let d = self.clone();
            let query = query.trim().to_string();
            let timer = glib::timeout_add_local_once(QALC_DELAY, move || {
                d.s.borrow_mut().qalc_timer = None;
                d.start_qalc(query, generation);
            });
            self.s.borrow_mut().qalc_timer = Some(timer);
        } else {
            self.s.borrow_mut().qalc_pending = false;
            self.show_calc(None);
        }
    }

    /// O qalc fora da linha da interface. A expressão vai como argumento, depois do --: nunca pelo shell nem
    /// como opção. Sem resposta no prazo, ele é encerrado
    fn start_qalc(self: &Rc<Self>, query: String, generation: u64) {
        let argv = [OsStr::new("qalc"), OsStr::new("-t"), OsStr::new("--"), OsStr::new(&query)];
        let flags = gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE;
        let proc = match gio::Subprocess::newv(&argv, flags) {
            Ok(proc) => proc,
            Err(err) => {
                warn(&format!("could not run qalc: {}", err.message()));
                self.qalc_done(generation, None);
                return;
            }
        };
        self.s.borrow_mut().qalc_proc = Some(proc.clone());
        let d = self.clone();
        glib::spawn_future_local(async move {
            // O prazo: o SourceId só é removido se ainda não disparou (remover um que já rodou é erro no GLib)
            let fired = Rc::new(Cell::new(false));
            let timeout = {
                let (proc, fired) = (proc.clone(), fired.clone());
                glib::timeout_add_local_once(QALC_TIMEOUT, move || {
                    fired.set(true);
                    proc.force_exit();
                })
            };
            let output = proc.communicate_utf8_future(None).await;
            if !fired.get() {
                timeout.remove();
            }
            {
                let mut s = d.s.borrow_mut();
                if s.qalc_proc.as_ref() == Some(&proc) {
                    s.qalc_proc = None;
                }
            }
            let text = match output {
                Ok((stdout, _)) => {
                    let success = proc.has_exited() && proc.exit_status() == 0;
                    logic::qalc_result(stdout.as_deref().unwrap_or(""), success)
                }
                Err(_) => None,
            };
            d.qalc_done(generation, text);
        });
    }

    fn qalc_done(&self, generation: u64, text: Option<String>) {
        {
            let mut s = self.s.borrow_mut();
            if generation != s.qalc_generation {
                return; // o texto já mudou: vale a consulta nova
            }
            s.qalc_pending = false;
        }
        self.show_calc(text);
        self.update_empty();
    }

    fn copy_result(&self) {
        // Esperando o qalc, o resultado à vista é o do texto anterior
        if self.s.borrow().qalc_pending {
            return;
        }
        let text = self.s.borrow().calc_text.clone();
        if let Some(text) = text {
            if run_detached(&["wl-copy", "--", &text]) {
                self.hide();
            }
        }
    }

    // Arquivos: em outra linha, para não travar a digitação

    fn schedule_files(self: &Rc<Self>, query: &str) {
        self.file_generation.fetch_add(1, Ordering::SeqCst);
        let timer = self.s.borrow_mut().file_timer.take(); // solto antes da chamada ao GLib
        if let Some(timer) = timer {
            timer.remove();
        }
        self.show_files(&[]);
        // update_calc já fez a conta (ou a pediu ao qalc): uma conta não procura arquivos
        let calc = {
            let s = self.s.borrow();
            s.calc_text.is_some() || s.qalc_pending
        };
        if logic::normalize(query).chars().count() < logic::MIN_FILE_QUERY || calc {
            return;
        }
        let generation = self.file_generation.load(Ordering::SeqCst);
        let d = self.clone();
        let query = query.to_string();
        let timer = glib::timeout_add_local_once(FILE_DELAY, move || {
            d.s.borrow_mut().file_timer = None;
            d.start_files(query, generation);
        });
        self.s.borrow_mut().file_timer = Some(timer);
    }

    fn start_files(self: &Rc<Self>, query: String, generation: u64) {
        // Sem o ~/.config/user-dirs.dirs (uma conta nova num Sway puro, sem o autostart que roda o
        // xdg-user-dirs-update), o GLib não sabe as pastas: valem os nomes padrão do xdg-user-dirs
        let folders: Vec<Option<PathBuf>> = [
            (glib::UserDirectory::Desktop, "Desktop"),
            (glib::UserDirectory::Documents, "Documents"),
            (glib::UserDirectory::Downloads, "Downloads"),
            (glib::UserDirectory::Music, "Music"),
            (glib::UserDirectory::Pictures, "Pictures"),
            (glib::UserDirectory::Videos, "Videos"),
        ]
        .into_iter()
        .map(|(kind, name)| glib::user_special_dir(kind).or_else(|| Some(glib::home_dir().join(name))))
        .collect();
        let roots = logic::search_roots(&glib::home_dir(), &folders);
        let current = self.file_generation.clone();
        let d = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let flag = current.clone();
            let found = gio::spawn_blocking(move || {
                let cancelled = || flag.load(Ordering::SeqCst) != generation;
                logic::find_files(&roots, &query, &cancelled, logic::FileLimits::default())
            })
            .await;
            match found {
                Ok(found) if current.load(Ordering::SeqCst) == generation => d.show_files(&found),
                Ok(_) => {}
                Err(_) => warn("the file search failed"),
            }
        });
    }

    fn show_files(self: &Rc<Self>, found: &[(PathBuf, bool)]) {
        if self.focus_in_files() {
            self.focus_entry(); // o botão com o foco vai sumir
        }
        remove_children(&self.w.files, |c| self.w.files.remove(c));
        // As pastas de busca vêm resolvidas
        let home = glib::home_dir().canonicalize().unwrap_or_else(|_| glib::home_dir());
        for (path, is_dir) in found {
            let button = gtk4::Button::new();
            button.add_css_class("file");
            button.set_focus_on_click(false); // o teclado chega a ele; o clique não tira o foco da busca
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let icon: gio::Icon = if *is_dir {
                gio::ThemedIcon::new("folder").upcast()
            } else {
                gio::content_type_get_icon(&gio::content_type_guess(Some(path), &[]).0)
            };
            row.append(&icon_image(Some(&icon), FILE_ICON_SIZE));
            let shown = match path.strip_prefix(&home) {
                Ok(rest) if rest.as_os_str().len() > 0 => format!("~/{}", rest.display()),
                _ => path.display().to_string(),
            };
            let text = label(&shown, None);
            text.set_xalign(0.0);
            text.set_ellipsize(pango::EllipsizeMode::Middle);
            text.set_max_width_chars(60); // sem isso, um caminho longo alarga a caixa além da busca
            row.append(&text);
            button.set_child(Some(&row));
            let d = self.clone();
            let (path, is_dir) = (path.clone(), *is_dir);
            button.connect_clicked(move |_| d.open_file(&path, is_dir));
            self.w.files.append(&button);
        }
        self.w.files.set_visible(!found.is_empty());
    }

    fn open_file(&self, path: &Path, is_dir: bool) {
        let ok = if is_dir {
            // O sway-files em Rust: o gerenciador da config (ou o primeiro instalado) com a pasta, solto; sem
            // ele instalado, o app padrão do sistema para pastas, como o script
            let installed = |name: &str| glib::find_program_in_path(name).is_some();
            let manager = swaydesk_common::config::file_manager(&swaydesk_common::config::load(), installed);
            match glib::find_program_in_path(&manager) {
                Some(program) => run_detached(&[program.as_os_str(), path.as_os_str()]),
                None => self.open_uri(path),
            }
        } else {
            self.open_uri(path)
        };
        if ok {
            self.hide();
        }
    }

    fn open_uri(&self, path: &Path) -> bool {
        let file = gio::File::for_path(path);
        let uri = file.uri();
        let context = launch_context();
        // O app padrão do arquivo abre pelo app_scope::launch (no seu escopo, sem corrida); sem um .desktop, o Gio
        // decide sozinho
        let handler = file.query_default_handler(gio::Cancellable::NONE).ok().and_then(|a| a.downcast::<gio::DesktopAppInfo>().ok());
        let opened = match handler {
            Some(info) => app_scope::launch(&info, None, &[uri.to_string()], Some(context.upcast_ref())),
            None => gio::AppInfo::launch_default_for_uri(&uri, Some(&context)),
        };
        match opened {
            Ok(()) => true,
            Err(err) => {
                warn(&format!("could not open {}: {}", path.display(), err.message()));
                run_app(&[OsStr::new("xdg-open"), path.as_os_str()], "xdg-open")
            }
        }
    }

    // Ações

    fn launch(&self, app_id: &str) {
        let (info, terminal) = {
            let s = self.s.borrow();
            let Some(info) = s.infos.get(app_id).cloned() else { return };
            let terminal = s.index.get(app_id).is_some_and(|&i| s.apps[i].fields.terminal);
            (info, terminal)
        };
        self.hide();
        // O Gio escolhe o terminal de uma lista própria, que pode não ter nenhum instalado: o terminal da config,
        // pelo sway-terminal (o mesmo do Ctrl+Alt+T)
        let program = terminal.then(|| {
            let helper = paths::find_helper("sway-terminal");
            logic::terminal_program(helper.as_deref(), glib::find_program_in_path("kitty").is_some())
        });
        if let Some(program) = program.flatten() {
            let commandline = info.commandline().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(argv) = logic::terminal_command(&commandline, &program) {
                if run_app(&argv, app_id) {
                    return;
                }
            }
        }
        let context = launch_context();
        if let Err(err) = app_scope::launch(&info, None, &[], Some(context.upcast_ref())) {
            warn(&format!("could not open {app_id}: {}", err.message()));
        }
    }

    fn on_entry_activate(&self) {
        // Sem busca não há nada marcado: o Enter não abre um app que o usuário não viu escolhido
        if let Some(i) = self.default_app() {
            let id = self.s.borrow().apps[i].id().to_string();
            self.launch(&id);
        } else {
            self.copy_result();
        }
    }

    /// Abre um dos menus no ponto do clique.
    fn popup_at(&self, menu: &gtk4::Popover, widget: &gtk4::Widget, x: f64, y: f64) {
        let point = widget.compute_point(&self.w.window, &graphene::Point::new(x as f32, y as f32));
        let (px, py) = point.map(|p| (p.x() as i32, p.y() as i32)).unwrap_or((0, 0));
        menu.set_pointing_to(Some(&gdk::Rectangle::new(px, py, 1, 1)));
        menu.popup();
    }

    fn open_menu(self: &Rc<Self>, app_id: &str, widget: &gtk4::Widget, x: f64, y: f64) {
        // Um app só da busca não se favorita, não se fixa na dock nem entra num grupo: ele não tem lugar
        // fora da busca, então não há menu
        if self.s.borrow().index.get(app_id).is_some_and(|&i| !self.s.borrow().apps[i].pinnable()) {
            return;
        }
        let favorite = self.s.borrow().pins.iter().any(|p| p == app_id);
        self.s.borrow_mut().menu_app = Some(app_id.to_string());
        // "Favoritar", e não "Fixar": fixar é na dock (o item de baixo)
        self.w.menu_button.set_label(&if favorite { tr("Remove from favorites") } else { tr("Add to favorites") });
        match self.pinned_in_dock(app_id) {
            None => {
                self.w.dock_button.set_visible(false);
                self.s.borrow_mut().dock_action = None;
            }
            Some(pinned) => {
                self.s.borrow_mut().dock_action = Some(if pinned { "unpin" } else { "pin" });
                self.w.dock_button.set_label(&if pinned { tr("Unpin from the dock") } else { tr("Pin to the dock") });
                self.w.dock_button.set_visible(true);
            }
        }
        // Os grupos, com uma marca nos que têm o app (no GTK 3, um submenu)
        remove_children(&self.w.groups_checks, |c| self.w.groups_checks.remove(c));
        let (entries, readable, in_group_view) = {
            let s = self.s.borrow();
            let app = s.index.get(app_id).map(|&i| &s.apps[i]);
            let entries: Vec<(String, bool)> = s
                .groups
                .iter()
                .map(|g| (groups::display_name(g, &group_label), app.is_some_and(|a| groups::in_group(a, g))))
                .collect();
            (entries, s.groups_readable, s.group.is_some())
        };
        for (index, (name, inside)) in entries.iter().enumerate() {
            let check = gtk4::CheckButton::with_label(name);
            check.set_active(*inside); // antes do connect: marcar não é escolher
            let d = self.clone();
            let id = app_id.to_string();
            check.connect_toggled(move |check| {
                d.w.menu.popdown();
                d.toggle_in_group(index, &id, check.is_active());
            });
            self.w.groups_checks.append(&check);
        }
        let show_groups = !entries.is_empty() && readable;
        self.w.groups_title.set_visible(show_groups);
        self.w.groups_checks.set_visible(show_groups);
        self.w.remove_button.set_visible(in_group_view && self.query().is_empty() && readable);
        self.popup_at(&self.w.menu, widget, x, y);
    }

    /// Se o app está fixado na dock, pela lista que a própria dock publica. None quando a opção não
    /// vale: a dock fora do ar (só ela grava os fixados dela), sem a lista (uma dock antiga, ou o
    /// arquivo dos fixados ilegível) ou com um pedido deste app que a dock ainda não publicou.
    fn pinned_in_dock(&self, app_id: &str) -> Option<bool> {
        let run = resident::runtime_dir();
        resident::live_pid(&run.join("sway-dock.pid"), "sway-dock")?;
        let waiting = self.s.borrow().dock_waiting.get(app_id).copied();
        match dock::read_pinned_ids(&run) {
            Ok(Some(ids)) => {
                let pinned = dock::is_pinned(&ids, app_id);
                // Até a dock publicar o estado pedido (ou o prazo): outra publicação, de outro app, não
                // conta, e o menu não oferece de novo o mesmo pedido
                if let Some((wanted, deadline)) = waiting {
                    if pinned != wanted && Instant::now() < deadline {
                        return None;
                    }
                    self.s.borrow_mut().dock_waiting.remove(app_id);
                }
                Some(pinned)
            }
            Ok(None) => None,
            Err(err) => {
                warn(&format!("could not read the dock's pinned apps: {err}"));
                None
            }
        }
    }

    /// O pedido vai pelo `sway-dock pin|unpin`: a dock grava e publica a lista nova.
    fn request_dock(&self, action: &str, app_id: &str) {
        if !dock::PIN_ACTIONS.contains(&action) {
            return;
        }
        let now = Instant::now();
        self.s.borrow_mut().dock_waiting.retain(|_, (_, deadline)| *deadline > now);
        let program = swaydesk_common::paths::component("sway-dock", "swaydesk-dock");
        if run_detached(&[program.as_os_str(), OsStr::new(action), OsStr::new(app_id)]) {
            self.s.borrow_mut().dock_waiting.insert(app_id.to_string(), (action == "pin", now + DOCK_WAIT));
        }
    }

    fn toggle_pin(self: &Rc<Self>, app_id: &str) {
        let pins = {
            let mut s = self.s.borrow_mut();
            if !s.pins_readable {
                drop(s);
                warn("the pinned apps could not be read: not saving over them");
                return;
            }
            s.pins = logic::toggle_pin(&s.pins, app_id);
            s.pins.clone()
        };
        if let Err(err) = logic::save_pins(&self.pins_path, &pins) {
            warn(&format!("could not save the pinned apps: {err}"));
        }
        self.build_pinned();
        self.apply_filter();
    }

    // Teclado e mouse

    /// O popover aberto: o menu do app, o de grupo ou o do nome.
    fn open_popover(&self) -> Option<gtk4::Popover> {
        [&self.w.menu, &self.w.group_menu, &self.w.name_popover].into_iter().find(|p| p.is_visible()).cloned()
    }

    fn on_key(self: &Rc<Self>, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        let stop = glib::Propagation::Stop;
        // O controlador da janela é de captura e vê antes as teclas dos popovers, que estão na árvore
        // dela: com um aberto, o Esc fecha só ele, e o resto é dele (o campo do nome)
        if let Some(popover) = self.open_popover() {
            if key == Key::Escape {
                popover.popdown();
                return stop;
            }
            return glib::Propagation::Proceed;
        }
        if state.contains(gdk::ModifierType::CONTROL_MASK) && (key == Key::Page_Up || key == Key::Page_Down) {
            self.cycle_group(if key == Key::Page_Up { -1 } else { 1 });
            return stop;
        }
        if key == Key::Escape {
            if self.w.entry.text().is_empty() {
                self.hide();
            } else {
                self.w.entry.set_text("");
            }
            return stop;
        }
        let down = key == Key::Down || key == Key::KP_Down;
        let up = key == Key::Up || key == Key::KP_Up;
        if self.entry_focused() {
            if (down || key == Key::Tab) && (self.focus_pinned() || self.focus_first() || self.focus_file(0)) {
                return stop;
            }
            return glib::Propagation::Proceed;
        }
        let plain = !state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::ALT_MASK);
        let focus = GtkWindowExt::focus(&self.w.window);
        let left = key == Key::Left || key == Key::KP_Left;
        let right = key == Key::Right || key == Key::KP_Right;
        if (left || right) && plain {
            // Para os lados na grade e nos favoritos, por conta própria: o FlowBox do GTK 4 não passa de
            // uma linha para a outra e, na ponta, mandava o foco para a busca ou para a grade de baixo
            if let Some(child) = focus.as_ref().and_then(|f| f.downcast_ref::<gtk4::FlowBoxChild>()).filter(|_| self.focus_in_grid()) {
                if let Some(target) = self.grid_sibling(child, right) {
                    target.grab_focus();
                }
                return stop;
            }
        }
        if (up || down) && plain {
            if let Some(child) = focus.as_ref().and_then(|f| f.downcast_ref::<gtk4::FlowBoxChild>()).filter(|_| self.focus_in_grid()) {
                // Para cima e para baixo na grade, por conta própria: o FlowBox não avisa quando não
                // há linha abaixo, nem desce de uma coluna sem item na última linha
                if !child.is_child_visible() {
                    if !(self.focus_pinned() || self.focus_first()) {
                        self.focus_entry();
                    }
                    return stop;
                }
                // Logo depois de filtrar (digitar e apertar ↓ na hora), as posições ainda são as do filtro
                // antigo: a tecla espera o próximo layout
                let since = self.s.borrow().filtered_at.map(|t| t.elapsed());
                if let Some(since) = since.filter(|s| *s < LAYOUT_WAIT) {
                    let d = self.clone();
                    // Parte do foco de quando roda, e não do item da tecla: dois ↓ seguidos viram dois
                    // timeouts, em ordem, e cada um anda um passo de onde o anterior parou. Se o foco saiu
                    // da grade (para a busca, um clique), não faz nada
                    glib::timeout_add_local_once(LAYOUT_WAIT - since, move || {
                        let Some(current) = GtkWindowExt::focus(&d.w.window)
                            .and_then(|f| f.downcast::<gtk4::FlowBoxChild>().ok())
                            .filter(|_| d.focus_in_grid())
                        else {
                            return;
                        };
                        match d.grid_neighbor(&current, down) {
                            Some(target) => {
                                target.grab_focus();
                            }
                            None => {
                                if let Some(grid) = current.parent().and_downcast::<gtk4::FlowBox>() {
                                    d.on_keynav_failed(&grid, if down { gtk4::DirectionType::Down } else { gtk4::DirectionType::Up });
                                }
                            }
                        }
                    });
                    return stop;
                }
                if let Some(target) = self.grid_neighbor(child, down) {
                    target.grab_focus();
                    return stop;
                }
                if let Some(grid) = child.parent().and_downcast::<gtk4::FlowBox>() {
                    let direction = if down { gtk4::DirectionType::Down } else { gtk4::DirectionType::Up };
                    self.on_keynav_failed(&grid, direction);
                }
                return stop;
            }
        }
        let files = self.file_buttons();
        if let Some(index) = focus.as_ref().and_then(|f| files.iter().position(|b| b.upcast_ref::<gtk4::Widget>() == f)) {
            if matches!(key, Key::Left | Key::Right | Key::KP_Left | Key::KP_Right) {
                return stop; // a lista é uma coluna: os lados não levam a lugar nenhum
            }
            if up || down {
                // As setas andam pela lista e, do primeiro, voltam à grade, no app de onde o teclado veio
                if down {
                    self.focus_file((index + 1).min(files.len() - 1));
                } else if index > 0 {
                    self.focus_file(index - 1);
                } else if !(self.focus_grid_return() || self.focus_last() || self.focus_pinned()) {
                    self.focus_entry();
                }
                return stop;
            }
        }
        // Digitar com o foco na grade continua a busca: as letras o GTK manda à busca sozinho
        // (set_key_capture_widget); o Backspace, não
        let modifiers = state & (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
        if modifiers.is_empty() && key == Key::BackSpace {
            let mut text = self.w.entry.text().to_string();
            text.pop();
            self.focus_entry();
            self.w.entry.set_text(&text);
            self.w.entry.set_position(-1);
            return stop;
        }
        glib::Propagation::Proceed
    }

    /// Os itens visíveis da grade, na ordem dela, com a posição (x, y) de cada um.
    fn shown_children(grid: &gtk4::FlowBox) -> Vec<(gtk4::FlowBoxChild, f32, f32)> {
        let mut shown = Vec::new();
        let mut next = grid.first_child();
        while let Some(widget) = next {
            next = widget.next_sibling();
            if let Ok(child) = widget.downcast::<gtk4::FlowBoxChild>() {
                if child.is_child_visible() {
                    let (x, y) = child.compute_bounds(grid).map(|b| (b.x(), b.y())).unwrap_or((0.0, 0.0));
                    shown.push((child, x, y));
                }
            }
        }
        shown.sort_by_key(|(c, _, _)| c.index()); // a ordem dos filhos não é a da ordenação
        shown
    }

    /// O item da linha de baixo (down) ou de cima, o mais perto na horizontal, ou None na borda da
    /// grade. Numa última linha mais curta, a coluna sem item cai no último dela, como no Python. Pela
    /// posição real dos itens: contar colunas pela largura dava outro número que o da grade.
    fn grid_neighbor(&self, child: &gtk4::FlowBoxChild, down: bool) -> Option<gtk4::FlowBoxChild> {
        let grid = child.parent().and_downcast::<gtk4::FlowBox>()?;
        let shown = Self::shown_children(&grid);
        let (_, x, y) = shown.iter().find(|(c, _, _)| c == child)?.clone();
        let rows = shown.iter().map(|(_, _, row_y)| *row_y).filter(|row_y| if down { *row_y > y + 1.0 } else { *row_y < y - 1.0 });
        let row = if down { rows.fold(None, |a: Option<f32>, b| Some(a.map_or(b, |a| a.min(b)))) } else { rows.fold(None, |a: Option<f32>, b| Some(a.map_or(b, |a| a.max(b)))) }?;
        shown
            .into_iter()
            .filter(|(_, _, row_y)| (row_y - row).abs() <= 1.0)
            .min_by(|(_, ax, _), (_, bx, _)| (ax - x).abs().total_cmp(&(bx - x).abs()))
            .map(|(c, _, _)| c)
    }

    /// O item anterior ou o seguinte na ordem da grade, passando de uma linha para a outra como no
    /// GTK 3; None nas pontas (aí o foco fica onde está, como no Python).
    fn grid_sibling(&self, child: &gtk4::FlowBoxChild, forward: bool) -> Option<gtk4::FlowBoxChild> {
        let grid = child.parent().and_downcast::<gtk4::FlowBox>()?;
        let shown: Vec<gtk4::FlowBoxChild> = Self::shown_children(&grid).into_iter().map(|(c, _, _)| c).collect();
        let here = shown.iter().position(|c| c == child)?;
        if forward { shown.get(here + 1).cloned() } else { here.checked_sub(1).and_then(|i| shown.get(i).cloned()) }
    }

    fn file_buttons(&self) -> Vec<gtk4::Button> {
        let mut buttons = Vec::new();
        if !self.w.files.is_visible() {
            return buttons;
        }
        let mut next = self.w.files.first_child();
        while let Some(widget) = next {
            next = widget.next_sibling();
            if let Ok(button) = widget.downcast::<gtk4::Button>() {
                buttons.push(button);
            }
        }
        buttons
    }

    fn focus_in_files(&self) -> bool {
        let Some(focus) = GtkWindowExt::focus(&self.w.window) else { return false };
        self.file_buttons().iter().any(|b| b.upcast_ref::<gtk4::Widget>() == &focus || focus.is_ancestor(b))
    }

    /// O arquivo da posição dada, na lista embaixo da grade. False: não há lista.
    fn focus_file(&self, index: usize) -> bool {
        let files = self.file_buttons();
        let Some(target) = files.get(index) else { return false };
        if !self.focus_in_files() {
            // Para voltar a ele
            let from = GtkWindowExt::focus(&self.w.window).and_then(|f| f.downcast::<gtk4::FlowBoxChild>().ok());
            self.s.borrow_mut().grid_return = from.filter(|c| self.s.borrow().children.contains_key(c));
        }
        target.grab_focus()
    }

    fn focus_grid_return(&self) -> bool {
        let child = self.s.borrow_mut().grid_return.take();
        match child {
            // Saiu da grade, ou o filtro o escondeu
            Some(child) if child.parent().is_some() && child.is_child_visible() => child.grab_focus(),
            _ => false,
        }
    }

    /// O último app da grade principal. False: não há nenhum.
    fn focus_last(&self) -> bool {
        let child = {
            let s = self.s.borrow();
            s.ranked.last().and_then(|&i| s.child_of.get(s.apps[i].id()).cloned())
        };
        child.is_some_and(|c| c.grab_focus())
    }

    /// O foco na busca, com o cursor no fim e sem selecionar o texto.
    fn focus_entry(&self) {
        self.w.entry.grab_focus();
        let end = self.w.entry.text().chars().count() as i32;
        self.w.entry.select_region(end, end);
    }

    /// O primeiro app da grade principal. False: não há nenhum.
    fn focus_first(&self) -> bool {
        let child = {
            let s = self.s.borrow();
            s.ranked.first().and_then(|&i| s.child_of.get(s.apps[i].id()).cloned())
        };
        child.is_some_and(|c| c.grab_focus())
    }

    /// O primeiro fixado, quando eles estão à vista. False: não estão.
    fn focus_pinned(&self) -> bool {
        if !self.w.pinned.is_visible() {
            return false;
        }
        self.w.pinned.first_child().is_some_and(|c| c.grab_focus())
    }

    fn on_keynav_failed(&self, grid: &gtk4::FlowBox, direction: gtk4::DirectionType) -> glib::Propagation {
        // Busca, favoritos, grade e arquivos: as setas passam de um para o outro
        let is_pinned = *grid == self.w.pinned;
        match direction {
            gtk4::DirectionType::Up => {
                if is_pinned || !self.focus_pinned() {
                    self.focus_entry();
                }
                glib::Propagation::Stop
            }
            gtk4::DirectionType::Down if is_pinned && (self.focus_first() || self.focus_file(0)) => glib::Propagation::Stop,
            gtk4::DirectionType::Down if !is_pinned => {
                self.focus_file(0); // sem arquivos, o foco fica onde está
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    /// Se o clique caiu fora do painel e de tudo que é clicável.
    fn on_background(&self, x: f64, y: f64) -> bool {
        let window: &gtk4::Widget = self.w.window.upcast_ref();
        let panel: &gtk4::Widget = self.w.panel.upcast_ref();
        let mut widget = window.pick(x, y, gtk4::PickFlags::DEFAULT);
        while let Some(current) = widget {
            if current == *window {
                break;
            }
            if current == *panel
                || current.is::<gtk4::Popover>()
                || current.is::<gtk4::Button>()
                || current.is::<gtk4::Text>()
                || current.is::<gtk4::SearchEntry>()
                || current.is::<gtk4::FlowBoxChild>()
                || current.is::<gtk4::Scrollbar>()
                || current.is::<gtk4::Popover>()
            {
                return false;
            }
            widget = current.parent();
        }
        true
    }

    /// A dock à vista fica fora da área de cliques da grade: um clique nela chega à dock (que abre o app
    /// e esconde a grade), em vez de cair no fundo e só fechar a grade.
    fn leave_dock_free(&self) {
        let Some(surface) = self.w.window.surface() else { return };
        let Some(monitor) = surface.display().monitor_at_surface(&surface) else { return };
        let g = monitor.geometry();
        let geometry = (g.x() as i64, g.y() as i64, g.width() as i64, g.height() as i64);
        let rect = |x: i64, y: i64, w: i64, h: i64| gtk4::cairo::RectangleInt::new(x as i32, y as i32, w as i32, h as i32);
        let region = gtk4::cairo::Region::create_rectangle(&rect(0, 0, surface.width() as i64, surface.height() as i64));
        for (x, y, w, h) in dock::holes(geometry, &dock::read_area(&resident::runtime_dir())) {
            if let Err(err) = region.subtract_rectangle(&rect(x, y, w, h)) {
                warn(&format!("could not leave the dock out of the grid's input: {err}"));
                return;
            }
        }
        surface.set_input_region(&region);
        // A região vai junto com o próximo quadro: sem um, uma mudança da dock com a grade parada não
        // chegaria ao Sway
        self.w.window.queue_draw();
    }

    // Mostrar e esconder

    pub fn toggle(self: &Rc<Self>) {
        if self.w.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    pub fn show(self: &Rc<Self>) {
        if self.s.borrow().apps_dirty {
            self.load_apps();
        }
        {
            let mut s = self.s.borrow_mut();
            s.group = None;
            s.hovered = None; // escondida, a janela pode não ter recebido a saída do mouse
        }
        self.w.entry.set_text("");
        self.apply_filter();
        self.w.scroller.vadjustment().set_value(0.0);
        self.fit_panel_before_show();
        self.w.window.set_visible(true);
        self.w.window.present();
        self.w.entry.grab_focus();
        // A dock só pede para esconder a grade (SIGUSR2) com este arquivo
        let shown = dock::shown_path(&resident::runtime_dir());
        if let Err(err) = std::fs::write(&shown, format!("{}\n", std::process::id())) {
            warn(&format!("could not write {}: {err}", shown.display()));
        }
    }

    pub fn hide(&self) {
        if !self.w.window.is_visible() {
            return;
        }
        self.w.menu.popdown();
        self.w.group_menu.popdown();
        self.w.name_popover.popdown();
        self.s.borrow_mut().grid_return = None;
        self.w.window.set_visible(false);
        remove_shown();
        self.file_generation.fetch_add(1, Ordering::SeqCst); // cancela uma busca de arquivos em andamento
        // O GTK solta o que usou para desenhar, mas a glibc guarda a memória livre no processo:
        // devolve ao sistema, um pouco depois, quando a janela já saiu da tela
        glib::timeout_add_local_once(Duration::from_secs(1), || {
            // SAFETY: malloc_trim só percorre as áreas livres do malloc
            unsafe { libc::malloc_trim(0) };
        });
        // Os .desktop mudaram com a grade aberta: refaz agora, escondida
        if self.s.borrow().apps_dirty {
            if let Some(me) = self.me.upgrade() {
                me.load_apps();
            }
        }
    }
}

pub fn remove_shown() {
    let shown = dock::shown_path(&resident::runtime_dir());
    if let Err(err) = std::fs::remove_file(&shown) {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn(&format!("could not remove {}: {err}", shown.display()));
        }
    }
}

fn watch_dock_area() -> Option<gio::FileMonitor> {
    let file = gio::File::for_path(dock::area_path(&resident::runtime_dir()));
    match file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
        Ok(watch) => Some(watch),
        Err(err) => {
            warn(&format!("could not watch the dock area: {err}"));
            None
        }
    }
}

fn launch_context() -> gdk::AppLaunchContext {
    let context = gdk::Display::default().expect("a display").app_launch_context();
    renderer::clean_launch_context(&context);
    app_scope::scope_launch_context(&context); // num serviço do usuário, cada app no seu escopo
    context
}

fn make_grid() -> gtk4::FlowBox {
    let grid = gtk4::FlowBox::new();
    grid.set_selection_mode(gtk4::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(COLUMNS);
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    grid.set_halign(gtk4::Align::Center);
    grid.set_valign(gtk4::Align::Start);
    grid
}

fn make_widgets() -> Widgets {
    let window = gtk4::Window::new();
    window.set_widget_name(PROGRAM);
    window.set_hide_on_close(true);
    window.init_layer_shell();
    // Acima da Waybar e da dock; o monitor é o do foco, escolhido pelo compositor
    window.set_layer(Layer::Overlay);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace(PROGRAM);

    // O painel no centro; o resto da janela é o fundo escurecido, e um clique nele fecha a grade
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    outer.set_widget_name("panel");
    outer.set_halign(gtk4::Align::Center);
    outer.set_valign(gtk4::Align::Center);
    window.set_child(Some(&outer));

    let entry = swaydesk_ui::search_entry(true, swaydesk_ui::SearchSize::Large);
    entry.set_size_request(SEARCH_WIDTH, -1);
    entry.set_halign(gtk4::Align::Center);
    entry.set_key_capture_widget(Some(&window));
    outer.append(&entry);

    let calc_button = gtk4::Button::new();
    calc_button.add_css_class("calc");
    calc_button.set_halign(gtk4::Align::Center);
    calc_button.set_focusable(false);
    calc_button.set_focus_on_click(false);
    calc_button.set_tooltip_text(Some(&tr("Copy the result")));
    calc_button.set_visible(false);
    outer.append(&calc_button);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    outer.append(&scroller);
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    column.set_halign(gtk4::Align::Center);
    scroller.set_child(Some(&column));

    let pinned = make_grid();
    pinned.set_widget_name("pinned");
    pinned.set_visible(false);
    column.append(&pinned);

    let grid = make_grid();
    column.append(&grid);

    let empty = label(&tr("No applications found"), Some("empty"));
    empty.set_visible(false);
    column.append(&empty);

    let files = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    files.set_widget_name("files");
    files.set_size_request(SEARCH_WIDTH, -1);
    files.set_halign(gtk4::Align::Center);
    files.set_visible(false);
    column.append(&files);

    let status = label("", Some("status"));
    status.set_ellipsize(pango::EllipsizeMode::End);
    outer.append(&status);

    outer.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    // Muitos grupos rolam para o lado, sem alargar o painel
    let bar = gtk4::ScrolledWindow::new();
    bar.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);
    bar.set_propagate_natural_height(true); // a largura não: é a do painel, e o resto rola
    let groups_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    groups_box.set_halign(gtk4::Align::Center);
    bar.set_child(Some(&groups_box));
    outer.append(&bar);

    let menu_item = |text: &str| {
        let button = gtk4::Button::with_label(text);
        button.set_focus_on_click(false);
        if let Some(label) = button.child().and_downcast::<gtk4::Label>() {
            label.set_xalign(0.0);
        }
        button
    };
    let new_menu = |child: &gtk4::Box| {
        let menu = gtk4::Popover::new();
        menu.add_css_class("menu");
        menu.set_has_arrow(false);
        menu.set_position(gtk4::PositionType::Bottom);
        menu.set_parent(&window);
        menu.set_child(Some(child));
        menu
    };

    // O menu de um app: favorito, dock, grupos e "Tirar deste grupo"
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let menu_button = menu_item("");
    column.append(&menu_button);
    let dock_button = menu_item("");
    column.append(&dock_button);
    let groups_title = label(&tr("Groups"), Some("section"));
    groups_title.set_xalign(0.0);
    column.append(&groups_title);
    let groups_checks = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    column.append(&groups_checks);
    let remove_button = menu_item(&tr("Remove from this group"));
    column.append(&remove_button);
    let menu = new_menu(&column);

    // O menu de um grupo
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let rename_button = menu_item(&tr("Rename"));
    column.append(&rename_button);
    let delete_button = menu_item(&tr("Delete group"));
    column.append(&delete_button);
    let group_menu = new_menu(&column);

    let name_popover = gtk4::Popover::new();
    name_popover.add_css_class("name");
    name_popover.set_position(gtk4::PositionType::Top);
    name_popover.set_parent(&window);
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let title = label(&tr("Group name"), None);
    title.set_xalign(0.0);
    column.append(&title);
    let name_entry = gtk4::Entry::new();
    name_entry.set_width_chars(24);
    column.append(&name_entry);
    name_popover.set_child(Some(&column));

    Widgets {
        window,
        panel: outer,
        entry,
        groups_box,
        calc_button,
        scroller,
        pinned,
        grid,
        empty,
        files,
        status,
        menu,
        menu_button,
        dock_button,
        groups_title,
        groups_checks,
        remove_button,
        group_menu,
        rename_button,
        delete_button,
        name_popover,
        name_entry,
    }
}
