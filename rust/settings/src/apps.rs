//! O painel Apps padrão: o app de cada categoria (navegador, e-mail, texto, imagens, PDF, vídeo, música) no
//! `~/.config/mimeapps.list`, só a camada do usuário; o padrão do sistema (o /etc/xdg das ISOs, com o Loupe e o
//! Papers) aparece como "Padrão do sistema (app)" e nunca é escrito. O gerenciador de arquivos e o terminal são
//! as chaves file_manager e terminal do config.toml: o sway-files, a dock, o Ctrl+Alt+E, o Ctrl+Alt+T e a grade as
//! leem, e o file_manager também vai para o sway-mimeapps.list gerado (que vence o mimeapps.list). As regras são
//! do swaydesk-settings-logic (apps, layers).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config::{self, Config, FILE_MANAGERS, TERMINALS};
use swaydesk_common::config_edit::{self, Edit, Value};
use swaydesk_common::i18n::tr;
use swaydesk_settings_logic::apps::{
    choices, mimeapps_files, set_defaults, system_default, types_for, user_default, write_file, Category, Choices, Label,
    CATEGORIES,
};
use swaydesk_settings_logic::desktop::Serial;
use swaydesk_settings_logic::layers::{self, Layers};

use crate::desktop::{desktop_config, heading, load, read, run_ok, with};
use crate::region::line;

/// O que uma linha muda.
#[derive(Clone, Copy)]
enum Kind {
    /// Os tipos de uma categoria, no mimeapps.list
    Mime(&'static Category),
    /// Uma chave do config.toml (file_manager, terminal), com os conhecidos (comando, .desktop)
    Key(&'static str, &'static [(&'static str, &'static str)]),
}

struct Row {
    kind: Kind,
    dropdown: gtk4::DropDown,
    /// O id de cada item da lista, na ordem dela (None: o padrão do sistema, ou o automático)
    ids: RefCell<Vec<Option<String>>>,
    labels: RefCell<Vec<String>>,
}

struct Panel {
    layers: RefCell<Layers>,
    message: gtk4::Label,
    rows: Vec<Row>,
    filling: Cell<bool>,
    /// Um generate por vez (o file_manager também vai para o sway-mimeapps.list)
    serial: RefCell<Serial>,
}

fn mimeapps_path() -> PathBuf {
    glib::user_config_dir().join("mimeapps.list")
}

fn installed(program: &str) -> bool {
    glib::find_program_in_path(program).is_some()
}

/// "pt_BR.UTF-8@x" -> "pt_BR"
fn base_locale(locale: &str) -> &str {
    locale.split('.').next().unwrap_or("").split('@').next().unwrap_or("")
}

/// A língua em que ler os nomes nos .desktop: a do `locale` do config.toml (a dos textos da janela) quando ela não é
/// a da sessão (LANGUAGE, LC_ALL, LC_MESSAGES, LANG). Vazia sem a chave, ou quando é a mesma: o Gio já dá o nome
/// nessa língua, e com o gettext do .desktop dos language packs do Ubuntu, que a leitura direta não tem.
fn name_languages(locale: &str, env: &HashMap<String, String>) -> Vec<String> {
    let locale = base_locale(locale);
    if locale.is_empty() {
        return Vec::new();
    }
    let session = config::message_languages(&Config::default(), env);
    if session.first().is_some_and(|first| base_locale(first) == locale) {
        return Vec::new();
    }
    vec![locale.to_string()]
}

/// A língua dos nomes, lida uma vez (o i18n lê o mesmo config ao abrir a janela).
fn config_languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(|| name_languages(&config::load().locale, &std::env::vars().collect()))
}

/// O nome do .desktop na primeira destas línguas que tiver tradução, como o Gio escolhe (o X-GNOME-FullName antes
/// do Name), ou o sem tradução. O Gio só segue o LANG/LANGUAGE do processo, e o locale do config pode ser outro
/// (trocado na Região, vale no próximo login). None sem língua (o Gio decide) ou sem o arquivo. Só nesse caso de
/// línguas diferentes, um .desktop sem a tradução no arquivo (o X-Ubuntu-Gettext-Domain dos language packs) fica
/// com o nome sem tradução.
fn localized_name(file: &Path, languages: &[String]) -> Option<String> {
    if languages.is_empty() {
        return None;
    }
    let keys = glib::KeyFile::new();
    keys.load_from_file(file, glib::KeyFileFlags::KEEP_TRANSLATIONS).ok()?;
    const GROUP: &str = "Desktop Entry";
    let key = ["X-GNOME-FullName", "Name"].into_iter().find(|key| keys.has_key(GROUP, key).unwrap_or(false))?;
    languages
        .iter()
        .find(|language| keys.locale_for_key(GROUP, key, Some(language.as_str())).is_some())
        .map_or_else(|| keys.string(GROUP, key), |language| keys.locale_string(GROUP, key, Some(language.as_str())))
        .ok()
        .map(|name| name.to_string())
}

/// O nome de um app, na língua da janela.
fn name_of(app: &impl IsA<gio::AppInfo>) -> String {
    app.dynamic_cast_ref::<gio::DesktopAppInfo>()
        .and_then(|desktop| desktop.filename())
        .and_then(|file| localized_name(&file, config_languages()))
        .unwrap_or_else(|| app.display_name().to_string())
}

/// O nome de um app pelo .desktop, ou o próprio id/comando.
fn app_name(desktop: &str) -> String {
    gio::DesktopAppInfo::new(&format!("{desktop}.desktop"))
        .or_else(|| gio::DesktopAppInfo::new(desktop))
        .map(|info| name_of(&info))
        .unwrap_or_else(|| desktop.trim_end_matches(".desktop").to_string())
}

impl Panel {
    fn show_error(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(true);
    }

    fn reload(self: &Rc<Self>) {
        *self.layers.borrow_mut() = load();
        self.fill();
    }

    /// A lista e o item marcado de uma linha, como estão agora (o mimeapps.list, o config.toml e os apps).
    fn list_for(&self, kind: Kind, mimeapps: &str, system_layers: &[String]) -> Choices {
        match kind {
            Kind::Mime(category) => {
                let primary = category.types[0];
                let user = user_default(mimeapps, primary);
                let candidates: Vec<(String, String)> = gio::AppInfo::all_for_type(primary)
                    .into_iter()
                    .filter(|app| app.should_show())
                    .filter_map(|app| Some((app.id()?.to_string(), name_of(&app))))
                    .collect();
                // O padrão do sistema, sem a escolha do usuário: o da camada do sistema que o dá (um app que existe),
                // ou, como no Gio, o primeiro que abre o tipo. Não muda com a escolha: trocar o nome do primeiro item
                // dentro do selected-notify deixava a lista aberta e a janela sem desenhar
                let system = system_default(system_layers, primary)
                    .and_then(|id| gio::DesktopAppInfo::new(&id))
                    .map(|app| name_of(&app))
                    .or_else(|| candidates.first().map(|(_, name)| name.clone()));
                choices(user.as_deref(), system.as_deref(), &candidates)
            }
            Kind::Key(key, known) => {
                let layers = self.layers.borrow();
                let user = layers.value(key).filter(|v| !v.is_empty());
                // O automático: o que o desktop-config daria sem a chave
                let empty = Config::default();
                let automatic = match key {
                    "terminal" => config::terminal(&empty, installed),
                    _ => config::file_manager(&empty, installed),
                };
                let auto_name = known.iter().find(|(n, _)| *n == automatic).map_or(automatic.clone(), |(_, d)| app_name(d));
                let candidates: Vec<(String, String)> =
                    known.iter().filter(|(n, _)| installed(n)).map(|(n, d)| (n.to_string(), app_name(d))).collect();
                choices(user.as_deref(), Some(&auto_name), &candidates)
            }
        }
    }

    fn fill(&self) {
        self.filling.set(true);
        let editable = self.layers.borrow().user_error.is_none();
        match &self.layers.borrow().user_error {
            Some(err) => {
                let first = err.lines().next().unwrap_or_default();
                self.show_error(&with(tr("Your config.toml has an error. Fix it to change these settings:\n%(error)s"), "error", first));
            }
            None => self.message.set_visible(false),
        }
        let mimeapps = read(&mimeapps_path()).unwrap_or_default();
        let desktops = std::env::var("XDG_CURRENT_DESKTOP").ok();
        let system_layers: Vec<String> = mimeapps_files(desktops.as_deref(), &glib::system_config_dirs(), &glib::system_data_dirs())
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .collect();
        for row in &self.rows {
            let list = self.list_for(row.kind, &mimeapps, &system_layers);
            let labels: Vec<String> = list
                .items
                .iter()
                .map(|item| match (&item.label, row.kind) {
                    (Label::System(Some(name)), Kind::Mime(_)) => with(tr("System default (%(app)s)"), "app", name),
                    (Label::System(None), Kind::Mime(_)) => tr("None"),
                    (Label::System(Some(name)), Kind::Key(..)) => with(tr("Automatic (%(app)s)"), "app", name),
                    (Label::System(None), Kind::Key(..)) => tr("Automatic"),
                    (Label::App(name), _) => name.clone(),
                })
                .collect();
            // O modelo só muda se a lista mudar: trocá-lo dentro do selected-notify deixa a lista aberta
            if *row.labels.borrow() != labels {
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                row.dropdown.set_model(Some(&gtk4::StringList::new(&refs)));
                *row.labels.borrow_mut() = labels;
            }
            if row.dropdown.selected() != list.selected as u32 {
                row.dropdown.set_selected(list.selected as u32);
            }
            *row.ids.borrow_mut() = list.items.into_iter().map(|item| item.id).collect();
            row.dropdown.set_sensitive(editable || matches!(row.kind, Kind::Mime(_)));
        }
        self.filling.set(false);
    }

    fn save(self: &Rc<Self>, kind: Kind, id: Option<String>) {
        let result = match kind {
            Kind::Mime(category) => save_mime(category, id.as_deref()),
            Kind::Key(key, _) => save_key(key, id.as_deref()),
        };
        self.reload();
        if let Err(err) = result {
            self.show_error(&with(tr("Could not save:\n%(error)s"), "error", &err));
            return;
        }
        if matches!(kind, Kind::Key("file_manager", _)) && desktop_config().is_some() && self.serial.borrow_mut().request() {
            self.spawn_generate();
        }
    }

    /// O generate do desktop-config, que leva o file_manager ao sway-mimeapps.list (as pastas abertas por outros
    /// apps); um por vez, e uma escolha durante ele pede mais um.
    fn spawn_generate(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let ok = gio::spawn_blocking(|| desktop_config().is_some_and(|program| run_ok(&program, &["generate"])))
                .await
                .unwrap_or(false);
            let Some(panel) = weak.upgrade() else { return };
            if !ok {
                panel.show_error(&tr("The file manager was saved, but folders opened by other apps may still use the old one"));
            }
            if panel.serial.borrow_mut().finished() {
                panel.spawn_generate();
            }
        });
    }
}

/// O app de uma categoria no mimeapps.list: todos os tipos dela saem, e o app escolhido fica só com os que ele
/// abre (o MimeType do .desktop); None tira a escolha, e vale o padrão do sistema.
fn save_mime(category: &Category, id: Option<&str>) -> Result<(), String> {
    let path = mimeapps_path();
    let text = read(&path)?;
    let cleared = set_defaults(&text, category.types, None);
    let text = match id {
        None => cleared,
        Some(id) => {
            let opens: Vec<String> = gio::DesktopAppInfo::new(id)
                .map(|info| info.supported_types().into_iter().map(|t| t.to_string()).collect())
                .unwrap_or_default();
            let opens: Vec<&str> = opens.iter().map(String::as_str).collect();
            let mut types = types_for(category, &opens);
            // Um candidato do Gio para o primeiro tipo pode tê-lo só por uma associação: ao menos ele
            if types.is_empty() {
                types.push(category.types[0]);
            }
            set_defaults(&cleared, &types, Some(id))
        }
    };
    write_file(&path, &text)
}

fn save_key(key: &str, value: Option<&str>) -> Result<(), String> {
    let edit = match value {
        None => Edit::Remove(key.to_string()),
        Some(value) => {
            layers::check(key, value)?;
            Edit::Set(key.to_string(), Value::Str(value.to_string()))
        }
    };
    config_edit::write(&config::path(), &[edit])
}

pub fn page() -> gtk4::Widget {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_halign(gtk4::Align::Center);
    content.set_width_request(560);

    let message = gtk4::Label::new(None);
    message.add_css_class("error");
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.set_visible(false);
    content.append(&message);
    content.append(&heading(&tr("Default apps"), &tr("Changes apply at once")));

    let category = |id: &str| CATEGORIES.iter().find(|c| c.id == id).expect("a category");
    // A ordem da página: navegador, e-mail, arquivos, terminal, texto, imagens, PDF, vídeo, música
    let kinds: Vec<(String, Kind)> = vec![
        (tr(category("web").name), Kind::Mime(category("web"))),
        (tr(category("mail").name), Kind::Mime(category("mail"))),
        (tr("File manager"), Kind::Key("file_manager", FILE_MANAGERS)),
        (tr("Terminal"), Kind::Key("terminal", TERMINALS)),
        (tr(category("text").name), Kind::Mime(category("text"))),
        (tr(category("images").name), Kind::Mime(category("images"))),
        (tr(category("pdf").name), Kind::Mime(category("pdf"))),
        (tr(category("video").name), Kind::Mime(category("video"))),
        (tr(category("music").name), Kind::Mime(category("music"))),
    ];
    let panel = Rc::new(Panel {
        layers: RefCell::new(Layers::default()),
        message,
        rows: kinds
            .iter()
            .map(|(_, kind)| Row {
                kind: *kind,
                dropdown: gtk4::DropDown::from_strings(&[]),
                ids: RefCell::new(Vec::new()),
                labels: RefCell::new(Vec::new()),
            })
            .collect(),
        filling: Cell::new(false),
        serial: RefCell::default(),
    });

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    for ((label, _), (index, row)) in kinds.iter().zip(panel.rows.iter().enumerate()) {
        let item = gtk4::ListBoxRow::new();
        item.set_activatable(false);
        item.set_child(Some(&line(label, "", &row.dropdown)));
        list.append(&item);
        let weak = Rc::downgrade(&panel);
        row.dropdown.connect_selected_notify(move |dropdown| {
            let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
            let row = &panel.rows[index];
            let Some(id) = row.ids.borrow().get(dropdown.selected() as usize).cloned() else { return };
            panel.save(row.kind, id);
        });
    }
    content.append(&list);

    panel.reload();
    let scroll = gtk4::ScrolledWindow::new();
    // Um app instalado com a janela aberta entra na lista na próxima visita
    let weak = Rc::downgrade(&panel);
    scroll.connect_map(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.reload();
        }
    });
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    // O painel vive enquanto a página viver: o tratador do destroy é dono dele (os outros só têm o Weak)
    scroll.connect_destroy(move |_| {
        let _ = &panel;
    });
    scroll.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop_file(dir: &std::path::Path, body: &str) -> PathBuf {
        let file = dir.join("viewer.desktop");
        std::fs::write(&file, format!("[Desktop Entry]\nType=Application\nExec=true\n{body}")).unwrap();
        file
    }

    fn langs(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// Uma pasta só do teste, apagada no fim dele (também quando falha)
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("swaydesk-settings-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    #[test]
    fn the_name_is_in_the_configs_language_not_the_environments() {
        let dir = scratch("lang");
        let file = desktop_file(&dir, "Name=Image Viewer\nName[pt_BR]=Visualizador\nName[es]=Visor de imágenes\n");
        assert_eq!(localized_name(&file, &langs(&["es_ES"])).as_deref(), Some("Visor de imágenes"));
        assert_eq!(localized_name(&file, &langs(&["es_ES.UTF-8"])).as_deref(), Some("Visor de imágenes"));
        assert_eq!(localized_name(&file, &langs(&["pt_BR"])).as_deref(), Some("Visualizador"));
    }

    #[test]
    fn without_a_translation_the_name_is_the_untranslated_one() {
        let dir = scratch("none");
        let file = desktop_file(&dir, "Name=Image Viewer\nName[pt_BR]=Visualizador\n");
        assert_eq!(localized_name(&file, &langs(&["fr"])).as_deref(), Some("Image Viewer"));
    }

    #[test]
    fn the_first_language_with_a_translation_wins() {
        let dir = scratch("order");
        let file = desktop_file(&dir, "Name=Image Viewer\nName[es]=Visor de imágenes\nName[de]=Bildbetrachter\n");
        assert_eq!(localized_name(&file, &langs(&["fr", "de", "es"])).as_deref(), Some("Bildbetrachter"));
    }

    #[test]
    fn the_full_name_comes_first_as_in_gio() {
        let dir = scratch("full");
        let file = desktop_file(&dir, "Name=Eye\nName[es]=Ojo\nX-GNOME-FullName=Eye of GNOME\nX-GNOME-FullName[es]=Ojo de GNOME\n");
        assert_eq!(localized_name(&file, &langs(&["es"])).as_deref(), Some("Ojo de GNOME"));
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_sessions_language_keeps_gios_name() {
        // O Gio já está nessa língua, e com os language packs do Ubuntu (o gettext do .desktop) acerta mais
        assert!(name_languages("pt_BR", &env(&[("LANG", "pt_BR.UTF-8")])).is_empty());
        assert!(name_languages("pt_BR.UTF-8", &env(&[("LANGUAGE", "pt_BR:en"), ("LANG", "en_US.UTF-8")])).is_empty());
    }

    #[test]
    fn another_language_reads_the_desktop_file() {
        assert_eq!(name_languages("es_ES", &env(&[("LANG", "pt_BR.UTF-8")])), langs(&["es_ES"]));
        assert_eq!(name_languages("pt_PT", &env(&[("LANG", "pt_BR.UTF-8")])), langs(&["pt_PT"]));
        assert_eq!(name_languages("es_ES", &env(&[("LC_ALL", "C")])), langs(&["es_ES"]));
    }

    #[test]
    fn without_a_locale_in_the_config_gio_decides() {
        assert!(name_languages("", &env(&[("LANG", "pt_BR.UTF-8")])).is_empty());
    }

    #[test]
    fn without_a_configured_language_gio_decides() {
        let dir = scratch("empty");
        let file = desktop_file(&dir, "Name=Image Viewer\nName[es]=Visor de imágenes\n");
        assert_eq!(localized_name(&file, &[]), None);
        assert_eq!(localized_name(&dir.join("missing.desktop"), &langs(&["es"])), None);
    }
}
