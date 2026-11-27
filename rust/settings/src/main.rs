//! A Central de Configurações (0.2.0): uma janela com a barra lateral dos painéis do swaydesk-settings-logic.
//! Os painéis próprios montam a página na primeira vez que aparecem; o Aparência e o Telas são as páginas dos
//! apps Tema e Telas (as bibliotecas do swaydesk-theme-app e do swaydesk-displays, ligadas estaticamente). Os de fora (rede, Bluetooth, som) abrem o app da
//! distro e só aparecem com ele instalado.
//!
//! Os argumentos são lidos antes do GTK: um `--panel` errado sai com 2 sem display nem barramento. A Central é
//! única (o GtkApplication no D-Bus da sessão): uma segunda chamada com `--panel` troca o painel da janela
//! aberta e volta com o código que a primeira devolveu.
mod about;
mod apps;
mod desktop;
mod input;
mod keyboard;
mod mouse;
mod power;
mod region;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config;
use swaydesk_common::i18n::{self, ptr, tr};
use swaydesk_settings_logic::{self as logic, ArgError, Kind, PanelInfo};
use swaydesk_ui::settings::Panel;

const APP_ID: &str = "org.swaydesktop.Settings"; // igual ao nome do .desktop: é como a dock acha o ícone

/// O conteúdo de cada painel próprio; os que ainda não existem são o aviso.
fn builder(id: &str) -> fn() -> gtk4::Widget {
    match id {
        "about" => about::page,
        "appearance" => swaydesk_theme_app::panel,
        "displays" => swaydesk_displays::panel,
        "default-apps" => apps::page,
        "desktop" => desktop::page,
        "keyboard" => keyboard::page,
        "mouse" => mouse::page,
        "power" => power::page,
        "region" => region::page,
        _ => placeholder,
    }
}

fn placeholder() -> gtk4::Widget {
    let label = gtk4::Label::new(Some(&tr("This panel is not available yet")));
    label.add_css_class("dim-label");
    label.set_vexpand(true);
    label.upcast()
}

/// A página de um painel de fora: o título e um botão que abre o app.
fn external_page(info: &'static PanelInfo, argv: &'static [&'static str]) -> gtk4::Widget {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    page.set_valign(gtk4::Align::Center);
    page.set_halign(gtk4::Align::Center);
    let icon = gtk4::Image::from_icon_name(info.icon);
    icon.set_pixel_size(64);
    let title = gtk4::Label::new(Some(&tr(info.title)));
    title.add_css_class("title-2");
    let button = gtk4::Button::with_label(&ptr("button", "Open"));
    button.connect_clicked(move |_| {
        if let Err(err) = swaydesk_ui::cmd::spawn_app(argv[0], &argv[1..]) {
            eprintln!("swaydesk-settings: {}", could_not_open(argv[0], &err));
        }
    });
    page.append(&icon);
    page.append(&title);
    page.append(&button);
    page.upcast()
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| logic::on_path(program, &path))
}

fn could_not_open(program: &str, err: &std::io::Error) -> String {
    tr("Could not open %(program)s: %(error)s").replace("%(program)s", program).replace("%(error)s", &err.to_string())
}

/// A mensagem de um erro de argumento, para o stderr.
fn arg_error(err: &ArgError) -> String {
    let text = match err {
        ArgError::UnknownPanel(name) => {
            let ids: Vec<&str> = logic::PANELS.iter().map(|p| p.id).collect();
            tr("unknown panel '%(name)s'; the panels are: %(panels)s").replace("%(name)s", name).replace("%(panels)s", &ids.join(", "))
        }
        ArgError::MissingValue => tr("--panel needs a panel name"),
        ArgError::UnknownArgument(arg) => format!("{}\n{}", tr("unknown argument '%(name)s'").replace("%(name)s", arg), tr("usage: swaydesk-settings [--panel <name>]")),
    };
    format!("swaydesk-settings: {text}")
}


struct Window {
    window: gtk4::ApplicationWindow,
    stack: gtk4::Stack,
}

impl Window {
    fn new(app: &gtk4::Application) -> Rc<Window> {
        let window = gtk4::ApplicationWindow::new(app);
        window.set_default_size(900, 620);
        let stack = gtk4::Stack::new();
        stack.set_hexpand(true);
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        for info in logic::visible_panels(installed) {
            let page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            if let Kind::External(argv) = info.kind {
                page.append(&external_page(info, argv));
            }
            stack.add_titled(&page, Some(info.id), &tr(info.title));
        }
        let sidebar = gtk4::StackSidebar::new();
        sidebar.set_stack(&stack);
        sidebar.set_width_request(220);
        let body = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        body.append(&sidebar);
        body.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));
        body.append(&stack);
        window.set_child(Some(&body));
        let this = Rc::new(Window { window, stack });
        let weak = Rc::downgrade(&this);
        this.stack.connect_visible_child_notify(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_panel_shown();
            }
        });
        this.on_panel_shown();
        this
    }

    /// O painel que aparece: monta o conteúdo na primeira vez e põe o nome dele no título da janela.
    fn on_panel_shown(&self) {
        let Some(id) = self.stack.visible_child_name() else { return };
        let Some(info) = logic::find(&id) else { return };
        if let Some(page) = self.stack.visible_child().and_then(|c| c.downcast::<gtk4::Box>().ok()) {
            if page.first_child().is_none() && info.kind == Kind::Builtin {
                let panel = Panel { id: info.id, build: builder(info.id) };
                page.append(&(panel.build)());
            }
        }
        self.window.set_title(Some(&format!("{} — {}", tr(info.title), tr("Settings"))));
    }

    fn show(&self, panel: Option<&str>) {
        if let Some(id) = panel {
            self.stack.set_visible_child_name(id);
        }
        self.window.present();
    }
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = "usage: swaydesk-settings [--panel <name>] [--about]
  --panel <name>  open on that panel (an unknown name lists them)
  --about         print the About panel's report and exit";

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    // As traduções antes de tudo: os erros de argumento também saem no idioma do usuário
    i18n::init(&config::load());
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    // Antes do GTK e do D-Bus: um argumento errado sai com 2 sem display
    let options = match logic::parse_args(&args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{}", arg_error(&err));
            return ExitCode::from(2);
        }
    };
    // O relato do Sobre num terminal: o mesmo texto do "Copiar informações", sem GTK nem display
    if options.about {
        print!("{}", about::report_text());
        return ExitCode::SUCCESS;
    }
    // Um painel que precisa de um programa ausente (o de um painel de fora, ou o pacote de que um próprio
    // depende) não abre: sai com 1, sem janela
    if let Some(id) = options.panel.filter(|id| logic::needs(id).is_some_and(|program| !installed(program))) {
        // O nome do pacote muda com a distro (o nm-connection-editor vem no network-manager-gnome do Debian)
        let os_release = std::fs::read_to_string("/etc/os-release").or_else(|_| std::fs::read_to_string("/usr/lib/os-release"));
        let family = os_release.ok().as_deref().and_then(logic::family);
        let package = logic::package_to_install(id, family).unwrap_or(id);
        eprintln!("swaydesk-settings: {}", tr("%(program)s is not installed").replace("%(program)s", package));
        return ExitCode::from(1);
    }
    // Um painel de fora pelo --panel: só abre o app, sem a janela da Central
    if let Some(info) = options.panel.and_then(logic::find) {
        if let Kind::External(argv) = info.kind {
            return match swaydesk_ui::cmd::spawn_app(argv[0], &argv[1..]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("swaydesk-settings: {}", could_not_open(argv[0], &err));
                    ExitCode::from(1)
                }
            };
        }
    }
    swaydesk_ui::use_cairo_renderer();
    // Sem isso, o app_id no Sway é o nome do programa, e a dock não acha o .desktop nem o ícone
    glib::set_prgname(Some(APP_ID));
    swaydesk_common::theme_resources::register_theme_resources();
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::HANDLES_COMMAND_LINE);
    // O adw-gtk3 num GTK antes do 4.20 deixa a janela clara até no escuro: o Adwaita embutido no lugar, e a janela
    // segue a troca do tema da sessão (os painéis não passam pelo main.rs dos apps, que fazem o mesmo)
    // E o destaque da config e a fonte do sistema nos widgets, como nos apps: a Central não depende de um painel
    // (o do Telas instala o mesmo CSS) para ter a cor
    app.connect_startup(|_| {
        swaydesk_ui::follow_theme_stand_in();
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&swaydesk_ui::theme_css());
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    });
    let window: Rc<RefCell<Option<Rc<Window>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, line| {
        // A primeira chamada e as seguintes (encaminhadas pelo D-Bus) passam por aqui: confere de novo, porque
        // a outra ponta pode ser outra versão do binário
        let args: Vec<String> = line.arguments().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        let panel = match logic::parse_args(&args) {
            // Só um painel que está na barra (um próprio com o que ele precisa instalado)
            Ok(options) => options.panel.filter(|id| logic::visible_panels(installed).iter().any(|p| p.id == *id && p.kind == Kind::Builtin)),
            Err(err) => {
                eprintln!("{}", arg_error(&err));
                return 2;
            }
        };
        let existing = window.borrow().clone();
        let current = match existing {
            Some(current) => current,
            None => {
                let created = Window::new(app);
                *window.borrow_mut() = Some(Rc::clone(&created));
                created
            }
        };
        current.show(panel);
        0
    });
    let code = app.run_with_args(&std::env::args().collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
