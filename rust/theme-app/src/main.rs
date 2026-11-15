//! Tema do Desktop: a família do tema GTK, claro ou escuro, e a cor de destaque, das cores do Papirus. O
//! porte de `home/.local/bin/sway-theme-app`, que passa a vez a este binário (sway_rust).
//!
//! A barra lateral tem as famílias (Mint-Y, Mint-L, Adwaita, Yaru...) e, depois delas, os outros temas. À
//! direita ficam a prévia, o estilo e as cores. Escolher só muda a prévia. O botão Aplicar, ou Enter,
//! aplica pela biblioteca do sway-theme (swaydesk-theme), e a janela continua aberta para testar outra
//! combinação: aplicar reinicia a barra. Abrir de novo traz a janela que já está aberta.
mod choice;
mod preview;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::process::ExitCode;
use std::rc::{Rc, Weak};
use std::time::Duration;

use choice::Choice;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use swaydesk_common::config::{self, load_config, papirus_color, Config, PAPIRUS_COLORS};
use swaydesk_common::i18n::{self, tr};
use swaydesk_theme::families::{self as sf, Family, Style};
use swaydesk_theme::themes::{self as st, Theme};
use swaydesk_theme::{desktop as dc, fill, fill_one, papirus as sp};

const APP_ID: &str = "org.swaydesktop.Theme"; // igual ao nome do .desktop: é como a dock acha o ícone
const SWATCH: i32 = 30;
const INFO_SECONDS: u32 = 8;
const CSS: &str = r#"
.theme-sidebar { border-right: 1px solid alpha(currentColor, 0.15); }
.theme-sidebar list { background: transparent; }
.theme-sidebar row { padding: 8px 12px; }
.theme-group { font-weight: bold; font-size: smaller; opacity: 0.7; padding: 14px 12px 4px 12px; }
.theme-section { font-weight: bold; }
.theme-swatches flowboxchild { padding: 2px; border-radius: 999px; }
.theme-swatches flowboxchild:selected { background: transparent; }
"#;

/// O círculo da cor, com uma marca quando é a escolhida.
fn swatch(name: &str) -> (gtk4::FlowBoxChild, gtk4::DrawingArea) {
    let child = gtk4::FlowBoxChild::new();
    child.set_tooltip_text(Some(&format!("{name} · {}", papirus_color(name).unwrap_or_default())));
    let area = gtk4::DrawingArea::new();
    area.set_content_width(SWATCH);
    area.set_content_height(SWATCH);
    let color = papirus_color(name).unwrap_or(config::DEFAULT_ACCENT).to_string();
    let weak_child = child.downgrade();
    area.set_draw_func(move |area, cr, width, height| {
        let (x, y) = (width as f64 / 2.0, height as f64 / 2.0);
        let radius = (width.min(height) as f64) / 2.0 - 3.0;
        let selected = weak_child.upgrade().is_some_and(|c| c.is_selected());
        let fg = area.color();
        cr.arc(x, y, radius, 0.0, std::f64::consts::TAU);
        preview::set_rgb(cr, &color);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.25);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        if selected {
            cr.arc(x, y, radius + 2.0, 0.0, std::f64::consts::TAU);
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
            preview::set_rgb(cr, &preview::on_accent(&color));
            cr.set_line_width(2.2);
            cr.move_to(x - radius * 0.38, y + radius * 0.02);
            cr.line_to(x - radius * 0.08, y + radius * 0.32);
            cr.line_to(x + radius * 0.42, y - radius * 0.3);
            let _ = cr.stroke();
        }
    });
    child.set_child(Some(&area));
    (child, area)
}

/// Uma linha da lateral: uma família, com as cores das variantes, ou um tema de fora delas.
struct Row {
    row: gtk4::ListBoxRow,
    family: Option<Family>,
    theme: Option<Theme>,
    label: String,
    /// Os nomes que a busca procura: o da linha e os das variantes
    names: Vec<String>,
    name: gtk4::Label,
    mark: gtk4::Image,
}

impl Row {
    fn new(family: Option<Family>, theme: Option<Theme>) -> Row {
        let label = family.as_ref().map(|f| f.name.clone()).or_else(|| theme.as_ref().map(|t| t.name.clone())).unwrap_or_default();
        let mut names = vec![label.clone()];
        if let Some(f) = &family {
            names.extend(f.variants.iter().map(|v| v.theme.clone()));
        }
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let name = gtk4::Label::new(Some(&label));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(pango::EllipsizeMode::End);
        top.append(&name);
        let mark = gtk4::Image::from_icon_name("object-select-symbolic");
        mark.set_visible(false);
        top.append(&mark);
        content.append(&top);
        let accents = match (&family, &theme) {
            (Some(f), _) => f.accents(),
            (None, Some(t)) => t.accent.iter().cloned().collect(),
            _ => Vec::new(),
        };
        if !accents.is_empty() {
            let dots = gtk4::DrawingArea::new();
            dots.set_content_height(10);
            let accents: Vec<String> = accents.into_iter().take(12).collect();
            dots.set_draw_func(move |_, cr, _, _| {
                for (index, color) in accents.iter().enumerate() {
                    cr.arc(5.0 + index as f64 * 12.0, 5.0, 4.0, 0.0, std::f64::consts::TAU);
                    preview::set_rgb(cr, color);
                    let _ = cr.fill();
                }
            });
            content.append(&dots);
        }
        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&content));
        Row { row, family, theme, label, names, name, mark }
    }

    fn set_current(&self, current: bool) {
        let attributes = pango::AttrList::new();
        if current {
            attributes.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
        }
        self.name.set_attributes(Some(&attributes));
        self.mark.set_visible(current);
        let tip = tr("In use");
        self.row.set_tooltip_text(if current { Some(tip.as_str()) } else { None });
    }
}

struct Window {
    window: gtk4::ApplicationWindow,
    me: Weak<Window>,
    choice: RefCell<Choice>,
    themes: RefCell<HashMap<String, Theme>>,
    papirus: Cell<bool>,
    colors: Vec<String>,
    config: RefCell<Config>,
    /// O tema da config, que pode não estar instalado
    wanted: RefCell<String>,
    /// O tema em uso
    current: RefCell<String>,
    current_color: RefCell<String>,
    note: RefCell<Option<String>>,
    icons_cache: RefCell<HashMap<String, String>>,
    applying: Cell<bool>,
    /// Evita que os sinais disparem de volta enquanto a janela é posta de acordo com a escolha
    updating: Cell<bool>,
    info_stamp: Cell<u64>,
    hide_source: RefCell<Option<glib::SourceId>>,
    config_changed: RefCell<Option<glib::SourceId>>,
    settings_handler: RefCell<Option<glib::SignalHandlerId>>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    apply_button: gtk4::Button,
    revert_button: gtk4::Button,
    spinner: gtk4::Spinner,
    entry: gtk4::SearchEntry,
    listbox: gtk4::ListBox,
    scroller: gtk4::ScrolledWindow,
    rows: RefCell<Vec<Row>>,
    #[allow(deprecated)] // sem substituto pronto no GTK 4.18; o do Python é o GtkInfoBar
    info: gtk4::InfoBar,
    info_label: gtk4::Label,
    preview: Rc<preview::Preview>,
    style_buttons: Vec<(Style, gtk4::ToggleButton)>,
    style_note: gtk4::Label,
    color_name: gtk4::Label,
    flowbox: gtk4::FlowBox,
    swatches: Vec<(String, gtk4::FlowBoxChild, gtk4::DrawingArea)>,
    summary: gtk4::Label,
    hint: gtk4::Label,
}

fn load_themes() -> (Vec<Theme>, Vec<Family>, Vec<Theme>) {
    let themes = st::discover();
    let (families, others) = sf::group(&themes);
    (themes, families, others)
}

fn margins(widget: &impl IsA<gtk4::Widget>, size: i32) {
    widget.set_margin_top(size);
    widget.set_margin_bottom(size);
    widget.set_margin_start(size);
    widget.set_margin_end(size);
}

fn dim(label: &gtk4::Label) {
    label.add_css_class("dim-label");
}

impl Window {
    #[allow(deprecated)]
    fn new(app: &gtk4::Application) -> Rc<Window> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let (themes, families, others) = load_themes();
        let mut colors = sp::available_colors(sp::theme_dir("Papirus").as_deref());
        if colors.is_empty() {
            colors = PAPIRUS_COLORS.iter().map(|(n, _)| n.to_string()).collect();
        }

        let window = gtk4::ApplicationWindow::new(app);
        window.set_title(Some(&tr("Desktop Theme")));
        window.set_default_size(860, 600);
        window.set_icon_name(Some("preferences-desktop-theme"));
        let header = gtk4::HeaderBar::new();
        window.set_titlebar(Some(&header));
        let apply_button = gtk4::Button::with_label(&tr("Apply"));
        apply_button.add_css_class("suggested-action");
        header.pack_end(&apply_button);
        let spinner = gtk4::Spinner::new();
        header.pack_end(&spinner);
        let revert_button = gtk4::Button::from_icon_name("edit-undo-symbolic");
        revert_button.set_tooltip_text(Some(&tr("Back to the theme in use")));
        header.pack_start(&revert_button);

        // A lateral
        let sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        sidebar.set_size_request(230, -1);
        sidebar.set_hexpand(false); // sem isso, o hexpand dos nomes sobe e a lateral divide a sobra com o resto
        sidebar.add_css_class("theme-sidebar");
        let entry = gtk4::SearchEntry::new();
        entry.set_placeholder_text(Some(&tr("Search themes")));
        margins(&entry, 8);
        sidebar.append(&entry);
        // Um clique só escolhe, para ver a prévia; aplicar reinicia a barra: duplo clique ou Enter
        let listbox = gtk4::ListBox::new();
        listbox.set_activate_on_single_click(false);
        listbox.set_selection_mode(gtk4::SelectionMode::Browse);
        let empty = gtk4::Label::new(Some(&tr("No theme found")));
        margins(&empty, 24);
        dim(&empty);
        listbox.set_placeholder(Some(&empty));
        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_vexpand(true);
        scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scroller.set_child(Some(&listbox));
        sidebar.append(&scroller);

        // O conteúdo
        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        outer.set_hexpand(true);
        let info = gtk4::InfoBar::new();
        info.set_show_close_button(true);
        let info_label = gtk4::Label::new(None);
        info_label.set_xalign(0.0);
        info_label.set_wrap(true);
        info_label.set_selectable(true);
        info.add_child(&info_label);
        info.set_visible(false);
        outer.append(&info);
        let content_scroller = gtk4::ScrolledWindow::new();
        content_scroller.set_vexpand(true);
        content_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        outer.append(&content_scroller);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        margins(&content, 20);
        content_scroller.set_child(Some(&content));

        let preview = preview::Preview::new();
        content.append(&preview.area);

        let style_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        style_row.set_margin_top(8);
        let style_label = gtk4::Label::new(Some(&tr("Style")));
        style_label.set_xalign(0.0);
        style_label.add_css_class("theme-section");
        style_row.append(&style_label);
        let toggles = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        toggles.add_css_class("linked");
        let mut style_buttons = Vec::new();
        for (style, text, icon) in [(Style::Light, tr("Light"), "weather-clear-symbolic"), (Style::Dark, tr("Dark"), "weather-clear-night-symbolic")] {
            let button = gtk4::ToggleButton::new();
            let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            inner.append(&gtk4::Image::from_icon_name(icon));
            inner.append(&gtk4::Label::new(Some(&text)));
            button.set_child(Some(&inner));
            toggles.append(&button);
            style_buttons.push((style, button));
        }
        style_row.append(&toggles);
        let style_note = gtk4::Label::new(None);
        style_note.set_xalign(0.0);
        style_note.set_wrap(true);
        style_note.set_hexpand(true);
        dim(&style_note);
        style_row.append(&style_note);
        content.append(&style_row);

        let color_title = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        color_title.set_margin_top(8);
        let color_label = gtk4::Label::new(Some(&tr("Accent color")));
        color_label.set_xalign(0.0);
        color_label.add_css_class("theme-section");
        color_title.append(&color_label);
        let color_name = gtk4::Label::new(None);
        color_name.set_xalign(0.0);
        dim(&color_name);
        color_title.append(&color_name);
        content.append(&color_title);
        let flowbox = gtk4::FlowBox::new();
        flowbox.set_selection_mode(gtk4::SelectionMode::Single);
        flowbox.set_homogeneous(true);
        flowbox.set_activate_on_single_click(true);
        flowbox.set_max_children_per_line(13);
        flowbox.set_min_children_per_line(6);
        flowbox.set_column_spacing(4);
        flowbox.set_row_spacing(4);
        flowbox.add_css_class("theme-swatches");
        flowbox.set_halign(gtk4::Align::Start);
        let mut swatches = Vec::new();
        for name in &colors {
            let (child, area) = swatch(name);
            flowbox.insert(&child, -1);
            swatches.push((name.clone(), child, area));
        }
        content.append(&flowbox);
        let papirus = sp::theme_dir("Papirus").is_some();
        if !papirus {
            let note = gtk4::Label::new(Some(&tr(
                "The Papirus icons are not installed: the color changes the accent, but not the folders. Install the papirus-icon-theme package.",
            )));
            note.set_xalign(0.0);
            note.set_wrap(true);
            dim(&note);
            content.append(&note);
        }
        let summary = gtk4::Label::new(None);
        summary.set_xalign(0.0);
        summary.set_wrap(true);
        summary.set_selectable(true);
        summary.set_margin_top(10);
        content.append(&summary);
        let hint = gtk4::Label::new(None);
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        dim(&hint);
        content.append(&hint);

        let paned = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        paned.append(&sidebar);
        paned.append(&outer);
        window.set_child(Some(&paned));

        let this = Rc::new_cyclic(|me| Window {
            window,
            me: me.clone(),
            choice: RefCell::new(Choice::new(families.clone(), others.clone(), &colors)),
            themes: RefCell::new(themes.into_iter().map(|t| (t.name.clone(), t)).collect()),
            papirus: Cell::new(papirus),
            colors,
            config: RefCell::new(Config::default()),
            wanted: RefCell::new(String::new()),
            current: RefCell::new(String::new()),
            current_color: RefCell::new(String::new()),
            note: RefCell::new(None),
            icons_cache: RefCell::new(HashMap::new()),
            applying: Cell::new(false),
            updating: Cell::new(false),
            info_stamp: Cell::new(0),
            hide_source: RefCell::new(None),
            config_changed: RefCell::new(None),
            settings_handler: RefCell::new(None),
            monitor: RefCell::new(None),
            apply_button,
            revert_button,
            spinner,
            entry,
            listbox,
            scroller,
            rows: RefCell::new(Vec::new()),
            info,
            info_label,
            preview,
            style_buttons,
            style_note,
            color_name,
            flowbox,
            swatches,
            summary,
            hint,
        });
        this.read_config();
        {
            let (current, color) = (this.current.borrow().clone(), this.current_color.borrow().clone());
            this.choice.borrow_mut().load(&current, &color);
        }
        this.fill_rows(&families, &others);
        this.connect();
        this.window.present();
        this.refresh(false);
        this
    }

    /// Um closure que chama `f` com a janela, se ela ainda existir.
    fn with<A>(&self, f: impl Fn(&Rc<Window>, A) + 'static) -> impl Fn(A) + 'static {
        let me = self.me.clone();
        move |arg| {
            if let Some(w) = me.upgrade() {
                f(&w, arg);
            }
        }
    }

    #[allow(deprecated)]
    fn connect(self: &Rc<Self>) {
        let apply = self.with(|w, ()| w.apply_selected());
        self.apply_button.connect_clicked(move |_| apply(()));
        let revert = self.with(|w, ()| w.revert());
        self.revert_button.connect_clicked(move |_| revert(()));

        let me = self.me.clone();
        self.listbox.set_filter_func(move |row| {
            let Some(w) = me.upgrade() else { return true };
            let text = w.entry.text();
            let rows = w.rows.borrow();
            rows.get(row.index() as usize).is_none_or(|r| choice::matches(&r.names, &text))
        });
        let me = self.me.clone();
        self.listbox.set_header_func(move |row, before| {
            let Some(w) = me.upgrade() else { return };
            let rows = w.rows.borrow();
            let is_family = |r: &gtk4::ListBoxRow| rows.get(r.index() as usize).is_some_and(|r| r.family.is_some());
            let family = is_family(row);
            let first_family = family && before.is_none();
            let first_other = !family && before.is_none_or(|b| is_family(b));
            if first_family || first_other {
                let label = gtk4::Label::new(Some(&tr(if first_family { "Themes" } else { "Other themes" })));
                label.set_xalign(0.0);
                label.add_css_class("theme-group");
                row.set_header(Some(&label));
            } else {
                row.set_header(None::<&gtk4::Widget>);
            }
        });
        let selected = self.with(|w, row: Option<gtk4::ListBoxRow>| w.on_row_selected(row));
        self.listbox.connect_row_selected(move |_, row| selected(row.cloned()));
        let activated = self.with(|w, ()| w.apply_selected());
        self.listbox.connect_row_activated(move |_, _| activated(()));

        let search = self.with(|w, ()| w.on_search());
        self.entry.connect_search_changed(move |_| search(()));
        // Enter só aplica o que a busca mostra: um tema escondido por ela não é o que se quer
        let activate = self.with(|w, ()| {
            if w.selection_visible() {
                w.apply_selected();
            }
        });
        self.entry.connect_activate(move |_| activate(()));
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = self.me.clone();
        keys.connect_key_pressed(move |_, key, _, _| match me.upgrade() {
            Some(w) if key == gdk::Key::Down || key == gdk::Key::Up => {
                w.move_selection(key == gdk::Key::Down);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        self.entry.add_controller(keys);

        // O Esc limpa a busca antes de fechar
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = self.me.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(w) = me.upgrade() else { return glib::Propagation::Proceed };
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if w.entry.text().is_empty() {
                w.window.close();
            } else {
                w.entry.set_text("");
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(keys);

        for (style, button) in &self.style_buttons {
            let style = *style;
            let toggled = self.with(move |w, active: bool| w.on_style_toggled(style, active));
            button.connect_toggled(move |b| toggled(b.is_active()));
        }
        let color = self.with(|w, ()| w.on_color_selected());
        self.flowbox.connect_selected_children_changed(move |_| color(()));

        let info = self.info.clone();
        self.info.connect_response(move |_, _| info.set_visible(false));

        // O tema também pode mudar fora daqui, com o sway-theme ou à mão. A cor das pastas muda só o
        // config.toml: o tema GTK pode continuar o mesmo
        if let Some(settings) = gtk4::Settings::default() {
            let changed = self.with(|w, ()| w.on_theme_changed());
            *self.settings_handler.borrow_mut() = Some(settings.connect_gtk_theme_name_notify(move |_| changed(())));
        }
        if let Ok(monitor) = gio::File::for_path(config::path()).monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            let changed = self.with(|w, ()| w.on_config_file_changed());
            monitor.connect_changed(move |_, _, _, _| changed(()));
            *self.monitor.borrow_mut() = Some(monitor);
        }
        let stop = self.with(|w, ()| w.stop_watching());
        self.window.connect_destroy(move |_| stop(()));
    }

    /// Na saída: um aviso atrasado do config.toml não pode atualizar widgets destruídos.
    fn stop_watching(&self) {
        if let Some(monitor) = self.monitor.borrow_mut().take() {
            monitor.cancel();
        }
        if let (Some(settings), Some(id)) = (gtk4::Settings::default(), self.settings_handler.borrow_mut().take()) {
            settings.disconnect(id);
        }
        for source in [&self.config_changed, &self.hide_source] {
            if let Some(id) = source.borrow_mut().take() {
                id.remove();
            }
        }
    }

    fn fill_rows(&self, families: &[Family], others: &[Theme]) {
        // Fora da lista antes de tirar: tirar uma linha refaz os títulos, que leem a lista
        let old = std::mem::take(&mut *self.rows.borrow_mut());
        for row in old {
            self.listbox.remove(&row.row);
        }
        let rows: Vec<Row> = families.iter().map(|f| Row::new(Some(f.clone()), None)).chain(others.iter().map(|t| Row::new(None, Some(t.clone())))).collect();
        // A lista antes das linhas: o filtro e os títulos leem a lista quando a linha entra
        let widgets: Vec<gtk4::ListBoxRow> = rows.iter().map(|r| r.row.clone()).collect();
        *self.rows.borrow_mut() = rows;
        for row in widgets {
            self.listbox.append(&row);
        }
    }

    /// Um tema instalado depois de abrir a janela, e aplicado pelo terminal: as linhas e o resumo o
    /// conhecem só depois de procurar de novo. Uma escolha que sumiu vira a em uso.
    fn reload_themes(&self) {
        let (themes, families, others) = load_themes();
        *self.themes.borrow_mut() = themes.into_iter().map(|t| (t.name.clone(), t)).collect();
        self.papirus.set(sp::theme_dir("Papirus").is_some());
        {
            let mut choice = self.choice.borrow_mut();
            let name = choice.family.as_ref().map(|f| f.name.clone());
            choice.families = families.clone();
            choice.others = others.clone();
            choice.family = families.iter().find(|f| Some(&f.name) == name.as_ref()).cloned();
            let other_gone = choice.other.as_ref().is_none_or(|o| !self.themes.borrow().contains_key(o));
            if choice.family.is_none() && other_gone {
                choice.load(&self.current.borrow(), &self.current_color.borrow());
            }
        }
        self.updating.set(true);
        self.fill_rows(&families, &others);
        self.updating.set(false);
    }

    fn read_config(&self) {
        let config = load_config(&config::path()).0;
        *self.wanted.borrow_mut() = config.gtk_theme.clone();
        *self.current.borrow_mut() = dc::effective_gtk_theme(&config);
        *self.note.borrow_mut() = st::missing_note(&config);
        // Sem o Papirus em uso, a cor mais perto do destaque, entre as que o app mostra
        *self.current_color.borrow_mut() = st::folder_color(&config).unwrap_or_else(|| sp::nearest_color(&config.accent, &self.colors));
        *self.config.borrow_mut() = config;
        self.icons_cache.borrow_mut().clear();
    }

    /// A escolha é o tema e a cor em uso. O destaque não entra: um que não é cor do Papirus (o padrão do
    /// config) deixaria o Aplicar e o Voltar ativos sem nenhuma mudança escolhida.
    fn applied(&self) -> bool {
        (Some(self.wanted.borrow().clone()), self.current_color.borrow().clone()) == self.choice.borrow().key()
    }

    /// Uma mudança de fora (terminal, config editado). Se a escolha era a aplicada, ela acompanha: senão
    /// o Aplicar ligaria e desfaria a mudança. Uma escolha em andamento fica.
    fn on_theme_changed(&self) {
        if self.applying.get() {
            return; // o fim da aplicação passa por aqui
        }
        self.sync(self.applied());
    }

    /// Relê o config e põe a janela de acordo. follow: a escolha passa a ser a aplicada.
    fn sync(&self, follow: bool) {
        self.read_config();
        let current = self.current.borrow().clone();
        if !self.themes.borrow().contains_key(&current) && dc::theme_installed(&current) {
            self.reload_themes();
        }
        if follow {
            let color = self.current_color.borrow().clone();
            self.choice.borrow_mut().load(&current, &color);
        }
        self.refresh(false);
    }

    /// Uma gravação dá vários eventos: espera eles acabarem.
    fn on_config_file_changed(&self) {
        if let Some(id) = self.config_changed.borrow_mut().take() {
            id.remove();
        }
        let me = self.me.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if let Some(w) = me.upgrade() {
                w.config_changed.borrow_mut().take();
                w.on_theme_changed();
            }
        });
        *self.config_changed.borrow_mut() = Some(id);
    }

    #[allow(deprecated)]
    fn revert(&self) {
        let (current, color) = (self.current.borrow().clone(), self.current_color.borrow().clone());
        self.choice.borrow_mut().load(&current, &color);
        self.info.set_visible(false);
        self.refresh(true);
    }

    /// Põe a janela de acordo com a escolha.
    fn refresh(&self, scroll: bool) {
        self.updating.set(true);
        let current = self.current.borrow().clone();
        let current_name = sf::family_of(&current).map(|(f, _)| f.to_string()).unwrap_or(current);
        let (family_name, other, style, color, theme, accent, has_family, exact, family_accents) = {
            let c = self.choice.borrow();
            (
                c.family.as_ref().map(|f| f.name.clone()),
                c.other.clone(),
                c.style,
                c.color.clone(),
                c.theme(),
                c.accent(),
                c.family.is_some(),
                c.exact(),
                c.family.as_ref().map(|f| f.accents().len()).unwrap_or(0),
            )
        };
        for row in self.rows.borrow().iter() {
            row.set_current(row.label == current_name);
            let chosen = (row.family.is_some() && family_name.as_deref() == Some(row.label.as_str()))
                || (row.theme.is_some() && other.as_deref() == Some(row.label.as_str()));
            if chosen {
                self.listbox.select_row(Some(&row.row));
                if scroll {
                    let me = self.me.clone();
                    let target = row.row.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(w) = me.upgrade() {
                            w.scroll_to(&target);
                        }
                    });
                }
            }
        }
        let name = family_name.clone().or_else(|| other.clone()).unwrap_or_default();
        for (s, button) in &self.style_buttons {
            let available = self.choice.borrow().has_style(*s);
            button.set_active(*s == style);
            button.set_sensitive(available && !self.applying.get());
            let tip = (!available).then(|| fill(&tr("%(theme)s has no %(style)s version"), &[("theme", &name), ("style", &s.label())]));
            button.set_tooltip_text(tip.as_deref());
        }
        self.style_note.set_text(&if has_family { String::new() } else { tr("This theme has a fixed style") });
        if let Some((_, child, _)) = self.swatches.iter().find(|(n, _, _)| *n == color) {
            self.flowbox.select_child(child);
        }
        for (_, _, area) in &self.swatches {
            area.queue_draw();
        }
        self.updating.set(false);
        self.color_name.set_text(&format!("{color} · {accent}"));
        let chosen = theme.as_ref().and_then(|t| self.themes.borrow().get(t).cloned());
        let icons = chosen.as_ref().map_or_else(|| "?".to_string(), |t| self.icons_for(t, &color));
        self.preview.set_choice(theme.clone(), style, &accent, &color);
        let mut parts = vec![theme.clone().unwrap_or_else(|| "?".into()), icons];
        if self.papirus.get() {
            parts.push(fill_one(&tr("%s folders"), &color));
        }
        self.summary.set_markup(&format!("<b>{}</b> {}", glib::markup_escape_text(&tr("Will apply:")), glib::markup_escape_text(&parts.join(" · "))));
        self.hint.set_text(&self.hint_text(theme.as_deref().unwrap_or(""), &family_name, &color, exact, family_accents, other.is_some()));
        self.update_buttons();
    }

    /// Os ícones que o apply vai gravar. Guardados por tema: o refresh roda a cada tecla da busca.
    fn icons_for(&self, chosen: &Theme, color: &str) -> String {
        if let Some(icons) = self.icons_cache.borrow().get(&chosen.name) {
            return icons.clone();
        }
        let icons = st::chosen_icons(chosen, &self.config.borrow(), Some(color));
        self.icons_cache.borrow_mut().insert(chosen.name.clone(), icons.clone());
        icons
    }

    /// O aviso de outro desktop vem antes de tudo, somado ao do tema.
    fn hint_text(&self, theme: &str, family: &Option<String>, color: &str, exact: bool, family_accents: usize, other: bool) -> String {
        let foreign = st::foreign_desktop_note_here().map(|note| format!("{note}."));
        let theme_hint = self.theme_hint(theme, family, color, exact, family_accents, other);
        foreign.into_iter().chain(Some(theme_hint)).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
    }

    fn theme_hint(&self, theme: &str, family: &Option<String>, color: &str, exact: bool, family_accents: usize, other: bool) -> String {
        if let Some(note) = self.note.borrow().as_ref() {
            return fill_one(&tr("%s. Choose a theme to save it in the config."), note);
        }
        if let Some(note) = st::kde_colors_note(theme) {
            return format!("{note}."); // o mais importante: o estilo escolhido não vale para os apps GTK 3
        }
        if let Some(family) = family {
            if !exact && family_accents > 0 {
                return fill(
                    &tr("%(family)s has no %(color)s variant: it uses %(theme)s. The bar, the folders and the GTK 4 apps get the exact color."),
                    &[("family", family), ("color", color), ("theme", theme)],
                );
            }
        }
        if (family.is_some() && family_accents == 0) || other {
            return tr("This theme keeps its own colors: the accent changes the bar, the folders and the GTK 4 apps.");
        }
        String::new()
    }

    fn update_buttons(&self) {
        let applied = self.applied();
        let ready = !self.applying.get() && self.choice.borrow().theme().is_some() && !applied;
        self.apply_button.set_sensitive(ready);
        self.revert_button.set_sensitive(!self.applying.get() && !applied);
    }

    fn on_row_selected(&self, row: Option<gtk4::ListBoxRow>) {
        let Some(row) = row else { return };
        if self.updating.get() {
            return;
        }
        if self.applying.get() {
            self.refresh(false); // a escolha é a que está sendo aplicada: a lista volta para ela
            return;
        }
        let picked = self.rows.borrow().get(row.index() as usize).map(|r| (r.family.clone(), r.theme.clone()));
        match picked {
            Some((Some(family), _)) => self.choice.borrow_mut().pick_family(&family),
            Some((None, Some(theme))) => self.choice.borrow_mut().pick_other(&theme),
            _ => return,
        }
        self.refresh(false);
    }

    fn on_style_toggled(&self, style: Style, active: bool) {
        if self.updating.get() {
            return;
        }
        if !self.applying.get() && active {
            self.choice.borrow_mut().style = style;
        }
        self.refresh(false);
    }

    fn on_color_selected(&self) {
        if self.updating.get() {
            return;
        }
        if self.applying.get() {
            self.refresh(false);
            return;
        }
        let selected = self.flowbox.selected_children();
        let Some(child) = selected.first() else { return };
        if let Some((name, _, _)) = self.swatches.iter().find(|(_, c, _)| c == child) {
            self.choice.borrow_mut().color = name.clone();
            self.refresh(false);
        }
    }

    fn visible_rows(&self) -> Vec<gtk4::ListBoxRow> {
        let text = self.entry.text();
        self.rows.borrow().iter().filter(|r| choice::matches(&r.names, &text)).map(|r| r.row.clone()).collect()
    }

    fn selection_visible(&self) -> bool {
        self.listbox.selected_row().is_some_and(|s| self.visible_rows().contains(&s))
    }

    /// O primeiro resultado vira a escolha, se a de antes sumiu: a prévia e o Enter vão com a busca.
    /// Limpar a busca só mostra todas as linhas: a escolha fica.
    fn on_search(&self) {
        self.listbox.invalidate_filter();
        if self.applying.get() {
            return;
        }
        let visible = self.visible_rows();
        if let Some(first) = visible.first() {
            if !self.selection_visible() {
                self.listbox.select_row(Some(first));
            }
        }
    }

    /// As setas na busca andam pelas linhas que ela mostra.
    fn move_selection(&self, down: bool) {
        let visible = self.visible_rows();
        if visible.is_empty() {
            return;
        }
        let selected = self.listbox.selected_row();
        let at = selected.and_then(|s| visible.iter().position(|r| *r == s)).map_or(-1, |i| i as i64);
        let at = (at + if down { 1 } else { -1 }).clamp(0, visible.len() as i64 - 1) as usize;
        self.listbox.select_row(Some(&visible[at]));
        self.scroll_to(&visible[at]);
    }

    fn scroll_to(&self, row: &gtk4::ListBoxRow) {
        let adjustment = self.scroller.vadjustment();
        let Some(bounds) = row.compute_bounds(&self.listbox) else { return };
        let (y, height) = (bounds.y() as f64, bounds.height() as f64);
        let (top, page) = (adjustment.value(), adjustment.page_size());
        if y < top || y + height > top + page {
            adjustment.set_value((y - page / 2.0).max(0.0));
        }
    }

    fn apply_selected(&self) {
        let action = self.choice.borrow().action();
        let theme = self.choice.borrow().theme();
        let (Some(action), Some(name)) = (action, theme) else { return };
        if self.applying.get() || self.applied() {
            return;
        }
        self.applying.set(true);
        self.spinner.start();
        self.update_buttons();
        for (_, button) in &self.style_buttons {
            button.set_sensitive(false);
        }
        #[allow(deprecated)]
        self.show_info(gtk4::MessageType::Info, &fill_one(&tr("Applying %s..."), &name), false);
        let me = self.me.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || choice::apply(&action)).await;
            let Some(w) = me.upgrade() else { return };
            match result {
                // 1: o tema foi aplicado, mas algum programa não acompanhou; 2: nada mudou, e a escolha fica
                // para tentar de novo. Sempre relê: uma mudança de fora durante a aplicação foi ignorada
                Ok((_, code, message)) => w.finish(&name, code == 0, &message, code != 2),
                Err(_) => w.finish(&name, false, &fill_one(&tr("Could not update: %s"), &name), false),
            }
        });
    }

    #[allow(deprecated)]
    fn finish(&self, name: &str, ok: bool, message: &str, follow: bool) {
        self.applying.set(false);
        self.spinner.stop();
        self.sync(follow);
        if ok {
            self.show_info(gtk4::MessageType::Info, &fill(&tr("%(theme)s applied. %(message)s"), &[("theme", name), ("message", message)]), true);
        } else {
            self.show_info(gtk4::MessageType::Warning, message, false);
        }
    }

    #[allow(deprecated)]
    fn show_info(&self, kind: gtk4::MessageType, text: &str, hide: bool) {
        self.info.set_message_type(kind);
        self.info_label.set_text(text);
        self.info.set_visible(true);
        // Uma mensagem nova cancela o fechamento da anterior
        let stamp = self.info_stamp.get() + 1;
        self.info_stamp.set(stamp);
        if let Some(id) = self.hide_source.borrow_mut().take() {
            id.remove();
        }
        if hide {
            let me = self.me.clone();
            let id = glib::timeout_add_seconds_local_once(INFO_SECONDS, move || {
                if let Some(w) = me.upgrade() {
                    w.hide_source.borrow_mut().take();
                    if w.info_stamp.get() == stamp && !w.applying.get() {
                        w.info.set_visible(false);
                    }
                }
            });
            *self.hide_source.borrow_mut() = Some(id);
        }
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o sway-theme-app em Python confere que o binário carrega
    }
    swaydesk_ui::use_cairo_renderer();
    // Sem isso, o app_id no Sway é o nome do programa, e a dock não acha o .desktop nem o ícone
    glib::set_prgname(Some(APP_ID));
    i18n::init(&config::load());
    swaydesk_common::theme_resources::register_theme_resources();
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::default());
    let window: Rc<RefCell<Option<Rc<Window>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        let existing = window.borrow().clone();
        match existing {
            Some(w) => w.window.present(),
            None => *window.borrow_mut() = Some(Window::new(app)),
        }
    });
    let code = app.run_with_args(&std::env::args().take(1).collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
