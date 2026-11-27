//! A página do Tema do Desktop, a mesma na janela do app (swaydesk-theme-app) e no painel Aparência da Central
//! de Configurações. Na janela, o Aplicar e o Voltar ficam na barra de título, e o Esc com a busca vazia fecha;
//! no painel, numa linha acima do conteúdo, e o Esc só limpa a busca.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use crate::choice::{Choice, Custom};
use crate::{choice, cursor, preview};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use swaydesk_common::config::{self, load_config, papirus_color, Config, PAPIRUS_COLORS};
use swaydesk_common::i18n::tr;
use swaydesk_theme::families::{self as sf, Family, Style};
use swaydesk_theme::themes::{self as st, Theme};
use swaydesk_theme::icon_themes::{self as it, IconTheme};
use swaydesk_theme::{desktop as dc, fill, fill_one, papirus as sp, wallpaper_accent as wa};

const SWATCH: i32 = 30;
const INFO_SECONDS: u32 = 8;
/// A faixa de ícones de cada tema na lista: os mais diferentes entre os temas, a pasta primeiro
const ICON_STRIP: [&str; 5] = ["folder", "user-home", "text-x-generic", "utilities-terminal", "preferences-system"];
const CSS: &str = r#"
.theme-sidebar { border-right: 1px solid alpha(currentColor, 0.15); }
.theme-sidebar list { background: transparent; }
.theme-sidebar row { padding: 8px 12px; }
.theme-group { font-weight: bold; font-size: smaller; opacity: 0.7; padding: 14px 12px 4px 12px; }
.theme-section { font-weight: bold; }
.theme-swatches flowboxchild { padding: 2px; border-radius: 999px; }
.theme-swatches flowboxchild:selected { background: transparent; }
.theme-swatches.theme-auto { opacity: 0.45; }
"#;
/// O botão marcado do Estilo na cor de destaque: no Adwaita-dark (o do GTK, e o que fica no lugar do adw-gtk3 antes do
/// 4.20), o marcado é só um cinza mais escuro que o vizinho, e o Escuro em uso parecia desmarcado. Depois do
/// config::accent_css, que define o @accent (o mesmo provider). O da versão em Python é o STYLE_TOGGLE_CSS dele.
const STYLE_TOGGLE_CSS: &str = "
.theme-style > button:checked:not(:disabled) { background-image: none; background-color: @accent; color: @on_accent; }
.theme-style > button:checked:hover:not(:disabled) { background-color: @accent_hover; }
";

/// O círculo da cor, com uma marca quando é a escolhida.
fn swatch(name: &str) -> (gtk4::FlowBoxChild, gtk4::DrawingArea) {
    let color = papirus_color(name).unwrap_or(config::DEFAULT_ACCENT).to_string();
    let (child, area) = swatch_of(Rc::new(RefCell::new(color)));
    child.set_tooltip_text(Some(&format!("{name} · {}", papirus_color(name).unwrap_or_default())));
    (child, area)
}

/// O círculo de uma cor que pode mudar (a amostra da cor própria).
fn swatch_of(color: Rc<RefCell<String>>) -> (gtk4::FlowBoxChild, gtk4::DrawingArea) {
    let child = gtk4::FlowBoxChild::new();
    let area = gtk4::DrawingArea::new();
    area.set_content_width(SWATCH);
    area.set_content_height(SWATCH);
    let weak_child = child.downgrade();
    area.set_draw_func(move |area, cr, width, height| {
        let color = color.borrow().clone();
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

pub(crate) struct Page {
    /// O conteúdo inteiro: o filho da janela do app, ou o painel da Central
    pub(crate) root: gtk4::Box,
    /// A janela do app; nenhuma no painel
    window: Option<gtk4::ApplicationWindow>,
    /// O CSS da página, tirado do display quando ela some (a Central continua aberta)
    provider: gtk4::CssProvider,
    me: Weak<Page>,
    choice: RefCell<Choice>,
    themes: RefCell<HashMap<String, Theme>>,
    papirus: Cell<bool>,
    colors: Vec<String>,
    config: RefCell<Config>,
    /// A lupa da busca na cor de destaque (config::accent_css): recarregado a cada leitura do config
    accent_provider: gtk4::CssProvider,
    /// O tema da config, que pode não estar instalado
    wanted: RefCell<String>,
    /// O tema em uso
    current: RefCell<String>,
    current_color: RefCell<String>,
    /// O accent_auto em uso
    current_auto: Cell<bool>,
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
    /// O texto da lista vazia: nenhum tema instalado, ou só a busca sem resultado (empty_text)
    empty: gtk4::Label,
    scroller: gtk4::ScrolledWindow,
    rows: RefCell<Vec<Row>>,
    #[allow(deprecated)] // sem substituto pronto no GTK 4.18; o do Python é o GtkInfoBar
    info: gtk4::InfoBar,
    info_label: gtk4::Label,
    preview: Rc<preview::Preview>,
    style_buttons: Vec<(Style, gtk4::ToggleButton)>,
    style_note: gtk4::Label,
    /// O aviso do adw-gtk3 com o snapd-desktop-integration, enquanto ele está escolhido (antes de Aplicar)
    snap_note: gtk4::Label,
    snaps: Cell<bool>,
    color_name: gtk4::Label,
    auto_switch: gtk4::Switch,
    auto_dot: gtk4::DrawingArea,
    auto_note: gtk4::Label,
    flowbox: gtk4::FlowBox,
    swatches: Vec<(String, gtk4::FlowBoxChild, gtk4::DrawingArea)>,
    /// A amostra da cor própria (choice::Custom), antes das outras; escondida sem ela
    custom_swatch: (gtk4::FlowBoxChild, gtk4::DrawingArea),
    custom_color: Rc<RefCell<String>>,
    /// A cor própria da config, com as pastas em uso
    current_custom: RefCell<Option<Custom>>,
    summary: gtk4::Label,
    hint: gtk4::Label,
    /// Os temas de ícones da lista, na ordem dela, e os ícones em uso (a entrada)
    icon_themes: RefCell<Vec<IconTheme>>,
    current_icons: RefCell<String>,
    icons_model: gtk4::StringList,
    icons_dropdown: gtk4::DropDown,
    /// O tamanho do cursor, numa seção própria que aplica ao escolher (cursor.rs)
    cursor_size: Rc<cursor::CursorSize>,
    /// Os temas de ícones da faixa, um por nome (a busca de cada ícone usa a herança do tema)
    icon_lookups: RefCell<HashMap<String, gtk4::IconTheme>>,
    /// O Papirus da faixa: o que contrasta com o fundo da janela, não o do estilo escolhido
    strip_papirus: RefCell<String>,
    folders_note: gtk4::Label,
    /// Por que a cor não vai para os gtk.css (st::gtk_css_status): fica à vista, não só depois de Aplicar
    css_note: gtk4::Label,
    css_notes: RefCell<Vec<String>>,
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

impl Page {
    /// Monta a página. Com a janela do app, os botões vão para a barra de título dela; sem, para uma linha
    /// acima do conteúdo. Falta o refresh: quem monta chama depois de mostrar (o fundo da janela decide a faixa)
    #[allow(deprecated)]
    pub(crate) fn new(window: Option<&gtk4::ApplicationWindow>) -> Rc<Page> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        let accent_provider = gtk4::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
            gtk4::style_context_add_provider_for_display(&display, &accent_provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let (themes, families, others) = load_themes();
        let mut colors = sp::available_colors(sp::theme_dir("Papirus").as_deref());
        if colors.is_empty() {
            colors = PAPIRUS_COLORS.iter().map(|(n, _)| n.to_string()).collect();
        }

        let apply_button = gtk4::Button::with_label(&tr("Apply"));
        apply_button.add_css_class("suggested-action");
        let spinner = gtk4::Spinner::new();
        let revert_button = gtk4::Button::from_icon_name("edit-undo-symbolic");
        revert_button.set_tooltip_text(Some(&tr("Back to the theme in use")));
        // No painel, a linha dos botões fica acima do conteúdo, à direita da lateral
        let actions = gtk4::CenterBox::new();
        if let Some(window) = window {
            let header = gtk4::HeaderBar::new();
            window.set_titlebar(Some(&header));
            header.pack_end(&apply_button);
            header.pack_end(&spinner);
            header.pack_start(&revert_button);
        } else {
            margins(&actions, 8);
            actions.set_start_widget(Some(&revert_button));
            let end = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            end.append(&spinner);
            end.append(&apply_button);
            actions.set_end_widget(Some(&end));
        }

        // A lateral
        let sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        sidebar.set_size_request(230, -1);
        sidebar.set_hexpand(false); // sem isso, o hexpand dos nomes sobe e a lateral divide a sobra com o resto
        sidebar.add_css_class("theme-sidebar");
        let entry = swaydesk_ui::search_entry(false, swaydesk_ui::SearchSize::Regular);
        margins(&entry, 8);
        sidebar.append(&entry);
        // Um clique só escolhe, para ver a prévia; aplicar reinicia a barra: duplo clique ou Enter
        let listbox = gtk4::ListBox::new();
        listbox.set_activate_on_single_click(false);
        listbox.set_selection_mode(gtk4::SelectionMode::Browse);
        let empty = gtk4::Label::new(Some(&tr("No theme found")));
        // A dica de instalar um tema é mais larga que a lateral
        empty.set_wrap(true);
        empty.set_justify(gtk4::Justification::Center);
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
        if window.is_none() {
            outer.append(&actions);
            outer.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
        }
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
        toggles.add_css_class("theme-style");
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
        let snap_note = gtk4::Label::new(None);
        snap_note.set_xalign(0.0);
        snap_note.set_wrap(true);
        snap_note.set_visible(false);
        dim(&snap_note);
        content.append(&snap_note);

        // Os ícones: a lista dos temas de ícones, cada um com o nome e uma faixa de ícones dele
        let icons_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let icons_label = gtk4::Label::new(Some(&tr("Icons")));
        icons_label.set_xalign(0.0);
        icons_label.add_css_class("theme-section");
        icons_row.append(&icons_label);
        let icons_model = gtk4::StringList::new(&[]);
        let icons_dropdown = gtk4::DropDown::new(Some(icons_model.clone()), None::<gtk4::Expression>);
        icons_label.set_mnemonic_widget(Some(&icons_dropdown));
        icons_row.append(&icons_dropdown);
        content.append(&icons_row);
        let cursor_size = cursor::CursorSize::new();
        content.append(&cursor_size.row);
        content.append(&cursor_size.note);

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
        // A cor do papel de parede: o interruptor, a amostra da cor que ele daria agora e o nome dela
        let auto_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        let auto_switch = gtk4::Switch::new();
        auto_switch.set_valign(gtk4::Align::Center);
        let auto_label = gtk4::Label::new(Some(&tr("Use the wallpaper's color")));
        auto_label.set_xalign(0.0);
        auto_label.set_mnemonic_widget(Some(&auto_switch));
        let auto_dot = gtk4::DrawingArea::new();
        auto_dot.set_content_width(18);
        auto_dot.set_content_height(18);
        auto_dot.set_valign(gtk4::Align::Center);
        let auto_note = gtk4::Label::new(None);
        auto_note.set_xalign(0.0);
        auto_note.set_wrap(true);
        auto_note.set_hexpand(true);
        dim(&auto_note);
        for widget in [auto_switch.upcast_ref::<gtk4::Widget>(), auto_label.upcast_ref(), auto_dot.upcast_ref(), auto_note.upcast_ref()] {
            auto_row.append(widget);
        }
        content.append(&auto_row);
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
        let custom_color = Rc::new(RefCell::new(config::DEFAULT_ACCENT.to_string()));
        let custom_swatch = swatch_of(custom_color.clone());
        custom_swatch.0.set_visible(false);
        flowbox.insert(&custom_swatch.0, -1);
        let mut swatches = Vec::new();
        for name in &colors {
            let (child, area) = swatch(name);
            flowbox.insert(&child, -1);
            swatches.push((name.clone(), child, area));
        }
        content.append(&flowbox);
        // Com outros ícones, a cor muda só o destaque
        let folders_note = gtk4::Label::new(None);
        folders_note.set_xalign(0.0);
        folders_note.set_wrap(true);
        dim(&folders_note);
        content.append(&folders_note);
        let css_note = gtk4::Label::new(None);
        css_note.set_xalign(0.0);
        css_note.set_wrap(true);
        css_note.set_visible(false);
        dim(&css_note);
        content.append(&css_note);
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

        let this = Rc::new_cyclic(|me| Page {
            root: paned,
            window: window.cloned(),
            provider,
            me: me.clone(),
            choice: RefCell::new(Choice::new(families.clone(), others.clone(), &colors)),
            themes: RefCell::new(themes.into_iter().map(|t| (t.name.clone(), t)).collect()),
            papirus: Cell::new(papirus),
            colors,
            config: RefCell::new(Config::default()),
            accent_provider,
            wanted: RefCell::new(String::new()),
            current: RefCell::new(String::new()),
            current_color: RefCell::new(String::new()),
            current_auto: Cell::new(false),
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
            empty,
            scroller,
            rows: RefCell::new(Vec::new()),
            info,
            info_label,
            preview,
            style_buttons,
            style_note,
            snap_note,
            snaps: Cell::new(false),
            color_name,
            auto_switch,
            auto_dot,
            auto_note,
            flowbox,
            swatches,
            custom_swatch,
            custom_color,
            current_custom: RefCell::new(None),
            summary,
            hint,
            icon_themes: RefCell::new(Vec::new()),
            current_icons: RefCell::new(String::new()),
            icons_model,
            icons_dropdown,
            cursor_size,
            icon_lookups: RefCell::new(HashMap::new()),
            strip_papirus: RefCell::new(String::new()),
            folders_note,
            css_note,
            css_notes: RefCell::new(Vec::new()),
        });
        this.set_icons_factory();
        // Depois do config: a língua dos nomes vem dele
        this.read_config();
        this.fill_icon_themes();
        this.load_choice();
        let me = this.me.clone();
        this.auto_dot.set_draw_func(move |area, cr, width, height| {
            let Some(w) = me.upgrade() else { return };
            let accent = w.choice.borrow().accent_for_auto();
            let (x, y, radius) = (width as f64 / 2.0, height as f64 / 2.0, width.min(height) as f64 / 2.0 - 1.0);
            cr.arc(x, y, radius, 0.0, std::f64::consts::TAU);
            preview::set_rgb(cr, &accent);
            let _ = cr.fill_preserve();
            let fg = area.color();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.25);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        });
        this.fill_rows(&families, &others);
        this.connect();
        this
    }

    /// Um closure que chama `f` com a janela, se ela ainda existir.
    fn with<A>(&self, f: impl Fn(&Rc<Page>, A) + 'static) -> impl Fn(A) + 'static {
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
        let icons = self.with(|w, ()| w.on_icons_selected());
        self.icons_dropdown.connect_selected_notify(move |_| icons(()));
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
            if !w.entry.text().is_empty() {
                w.entry.set_text("");
            } else if let Some(window) = &w.window {
                window.close();
            } else {
                return glib::Propagation::Proceed; // no painel, o Esc com a busca vazia é da Central
            }
            glib::Propagation::Stop
        });
        // Na janela, o Esc vale com o foco em qualquer lugar dela, a barra de título inclusive
        match &self.window {
            Some(window) => window.add_controller(keys),
            None => self.root.add_controller(keys),
        }

        for (style, button) in &self.style_buttons {
            let style = *style;
            let toggled = self.with(move |w, active: bool| w.on_style_toggled(style, active));
            button.connect_toggled(move |b| toggled(b.is_active()));
        }
        let color = self.with(|w, ()| w.on_color_selected());
        self.flowbox.connect_selected_children_changed(move |_| color(()));
        // Um clique numa amostra com o interruptor ligado também é escolher à mão: o selected-children-changed não
        // vem quando a amostra já era a marcada
        let clicked = self.with(|w, ()| w.on_color_selected());
        self.flowbox.connect_child_activated(move |_, _| clicked(()));
        let auto = self.with(|w, active: bool| w.on_auto_toggled(active));
        self.auto_switch.connect_active_notify(move |s| auto(s.is_active()));

        let info = self.info.clone();
        self.info.connect_response(move |_, _| info.set_visible(false));

        // O tema também pode mudar fora daqui, com o sway-theme ou à mão. A cor das pastas muda só o
        // config.toml: o tema GTK pode continuar o mesmo
        if let Some(settings) = gtk4::Settings::default() {
            swaydesk_ui::stand_in_theme(&settings);
            let changed = self.with(|w, ()| w.on_theme_changed());
            *self.settings_handler.borrow_mut() = Some(settings.connect_gtk_theme_name_notify(move |s| {
                swaydesk_ui::stand_in_theme(s);
                changed(())
            }));
            swaydesk_ui::follow_session_theme(&settings);
        }
        if let Ok(monitor) = gio::File::for_path(config::path()).monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            let changed = self.with(|w, ()| w.on_config_file_changed());
            monitor.connect_changed(move |_, _, _, _| changed(()));
            *self.monitor.borrow_mut() = Some(monitor);
        }
        let stop = self.with(|w, ()| w.stop_watching());
        self.root.connect_destroy(move |_| stop(()));
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
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_remove_provider_for_display(&display, &self.provider);
            gtk4::style_context_remove_provider_for_display(&display, &self.accent_provider);
        }
    }

    fn fill_rows(&self, families: &[Family], others: &[Theme]) {
        // Fora da lista antes de tirar: tirar uma linha refaz os títulos, que leem a lista
        let old = std::mem::take(&mut *self.rows.borrow_mut());
        for row in old {
            self.listbox.remove(&row.row);
        }
        let rows: Vec<Row> = families.iter().map(|f| Row::new(Some(f.clone()), None)).chain(others.iter().map(|t| Row::new(None, Some(t.clone())))).collect();
        self.empty.set_text(&empty_text(!rows.is_empty()));
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
        self.fill_icon_themes();
        self.papirus.set(sp::theme_dir("Papirus").is_some());
        {
            let mut choice = self.choice.borrow_mut();
            let name = choice.family.as_ref().map(|f| f.name.clone());
            choice.families = families.clone();
            choice.others = others.clone();
            choice.family = families.iter().find(|f| Some(&f.name) == name.as_ref()).cloned();
            let other_gone = choice.other.as_ref().is_none_or(|o| !self.themes.borrow().contains_key(o));
            if choice.family.is_none() && other_gone {
                choice.load(&self.current.borrow(), &self.current_color.borrow(), self.current_auto.get());
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
        // Sem o Papirus em uso, a cor mais perto do destaque, entre as que o app mostra; o destaque padrão exato
        // (que não é amostra), com o modo desligado, carrega como nenhuma ("")
        let folders = st::folder_color(&config).unwrap_or_else(|| sp::nearest_color(&config.accent, &self.colors));
        // O modo em vigor: sem a chave no config e sem o arquivo do papel, desligado (Config::accent_auto_for)
        let auto = config.accent_auto_for(wa::wallpaper().exists());
        *self.current_color.borrow_mut() = choice::loaded_color(&config.accent, &config.folder_color, auto, &folders);
        self.current_auto.set(auto);
        *self.css_notes.borrow_mut() = st::gtk_css_status();
        self.snaps.set(dc::snap_theme_prompt());
        // A cor do próprio tema (sway-theme set sem --accent) leva o nome dele; outra, "Custom"
        let own = self.themes.borrow().get(&*self.current.borrow()).and_then(|t| t.accent.clone());
        *self.current_custom.borrow_mut() = choice::custom_accent(&config.accent, auto, &config.folder_color, &folders).map(|accent| Custom {
            theme: own.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&accent)).then(|| self.current.borrow().clone()),
            accent,
            folders,
        });
        *self.current_icons.borrow_mut() = it::entry_of(&dc::installed_icon_theme(&config)).to_string();
        self.accent_provider.load_from_string(&format!("{}{STYLE_TOGGLE_CSS}", config::accent_css(&config::palette(&config))));
        self.cursor_size.load(&config);
        // Sem a chave no config, o interruptor desligado e intocado não grava o false no Aplicar
        self.choice.borrow_mut().auto_inferred = !config.accent_auto_set;
        *self.config.borrow_mut() = config;
        // O papel pode ter mudado (o do dia): a cor que o interruptor daria agora
        self.choice.borrow_mut().wallpaper = wa::current_color();
        self.auto_dot.queue_draw();
        self.icons_cache.borrow_mut().clear();
    }

    /// A escolha do tema, da cor, do modo e dos ícones em uso.
    fn load_choice(&self) {
        let (current, color) = (self.current.borrow().clone(), self.current_color.borrow().clone());
        let mut choice = self.choice.borrow_mut();
        choice.custom = self.current_custom.borrow().clone();
        choice.load(&current, &color, self.current_auto.get());
        choice.load_icons(&self.current_icons.borrow());
    }

    /// Os temas de ícones da lista, de novo (um instalado com a janela aberta). A escolha fica, se ainda existe.
    fn fill_icon_themes(&self) {
        let env: HashMap<String, String> = std::env::vars().collect();
        let lang = config::message_languages(&self.config.borrow(), &env).into_iter().next().unwrap_or_default();
        let themes = it::installed(&lang);
        let ids: Vec<String> = themes.iter().map(|t| t.id.clone()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        // Os nomes antes das linhas: o splice já desenha as linhas, que leem o nome daqui
        *self.icon_themes.borrow_mut() = themes;
        self.icon_lookups.borrow_mut().clear();
        self.updating.set(true);
        self.icons_model.splice(0, self.icons_model.n_items(), &ids);
        self.updating.set(false);
    }

    /// O nome na lista: o do index.theme; o do Papirus diz que segue o estilo.
    fn icon_label(&self, id: &str) -> String {
        if id == it::PAPIRUS {
            return tr("Papirus (light or dark, with the style)");
        }
        self.icon_themes.borrow().iter().find(|t| t.id == id).map_or_else(|| id.to_string(), |t| t.label.clone())
    }

    /// O nome de uns ícones no resumo e na nota: o rótulo da lista ("GNOME", não "gnome"). O Papirus fica com a
    /// variante (Papirus-Dark), que é o que vai ser gravado.
    fn icons_display(&self, name: &str) -> String {
        if sp::is_papirus(name) {
            return name.to_string();
        }
        self.icon_themes.borrow().iter().find(|t| t.id == name).map_or_else(|| name.to_string(), |t| t.label.clone())
    }

    /// O Papirus que se lê no fundo da janela agora: o escuro (ícones claros) com texto claro.
    fn papirus_for_window(&self) -> String {
        let fg = self.root.color();
        let light_text = 0.299 * fg.red() + 0.587 * fg.green() + 0.114 * fg.blue() > 0.5;
        let wanted = sf::icons_for_style(if light_text { Style::Dark } else { Style::Light });
        if dc::icons_installed(wanted) { wanted.to_string() } else { sp::THEMES.iter().find(|n| dc::icons_installed(n)).unwrap_or(&wanted).to_string() }
    }

    /// Com o fundo da janela trocado (outro tema GTK aplicado), as linhas já desenhadas refazem a faixa do Papirus.
    fn sync_strips(&self) {
        let name = self.papirus_for_window();
        if *self.strip_papirus.borrow() == name {
            return;
        }
        *self.strip_papirus.borrow_mut() = name;
        let n = self.icons_model.n_items();
        if n == 0 {
            return;
        }
        // Trocar as linhas pelas mesmas refaz o bind de cada uma (o queue_draw só redesenha as imagens antigas)
        let ids: Vec<String> = (0..n).filter_map(|i| self.icons_model.string(i)).map(|s| s.to_string()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let selected = self.icons_dropdown.selected();
        let updating = self.updating.replace(true);
        self.icons_model.splice(0, n, &ids);
        self.icons_dropdown.set_selected(selected);
        self.updating.set(updating);
    }

    /// A busca dos ícones de um tema da lista, sem o gerado nem as reservas: a faixa mostra o tema como ele é. O
    /// Papirus vem na variante que contrasta com o fundo da lista.
    fn icon_lookup(&self, id: &str) -> gtk4::IconTheme {
        let name = if id == it::PAPIRUS {
            let mut name = self.strip_papirus.borrow().clone();
            if name.is_empty() {
                name = self.papirus_for_window();
                *self.strip_papirus.borrow_mut() = name.clone();
            }
            name
        } else {
            id.to_string()
        };
        self.icon_lookups
            .borrow_mut()
            .entry(name.clone())
            .or_insert_with(|| {
                let theme = gtk4::IconTheme::new();
                theme.set_theme_name(Some(&name));
                theme
            })
            .clone()
    }

    /// Cada linha da lista: o nome e a faixa de ícones do tema (um ícone que ele não tem fica de fora).
    fn set_icons_factory(&self) {
        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk4::ListItem>() else { return };
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
            let label = gtk4::Label::new(None);
            label.set_xalign(0.0);
            row.append(&label);
            let strip = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            row.append(&strip);
            item.set_child(Some(&row));
        });
        let me = self.me.clone();
        factory.connect_bind(move |_, item| {
            let Some(w) = me.upgrade() else { return };
            let Some(item) = item.downcast_ref::<gtk4::ListItem>() else { return };
            let Some(id) = item.item().and_downcast::<gtk4::StringObject>().map(|o| o.string().to_string()) else { return };
            let Some(row) = item.child().and_downcast::<gtk4::Box>() else { return };
            let Some(label) = row.first_child().and_downcast::<gtk4::Label>() else { return };
            let Some(strip) = row.last_child().and_downcast::<gtk4::Box>() else { return };
            label.set_text(&w.icon_label(&id));
            while let Some(child) = strip.first_child() {
                strip.remove(&child);
            }
            let lookup = w.icon_lookup(&id);
            for name in ICON_STRIP.iter().filter(|n| lookup.has_icon(n)) {
                let paintable = lookup.lookup_icon(name, &[], 16, w.root.scale_factor(), gtk4::TextDirection::None, gtk4::IconLookupFlags::empty());
                let image = gtk4::Image::from_paintable(Some(&paintable));
                image.set_pixel_size(16);
                strip.append(&image);
            }
        });
        self.icons_dropdown.set_factory(Some(&factory));
    }

    fn on_icons_selected(&self) {
        if self.updating.get() || self.applying.get() {
            return;
        }
        let position = self.icons_dropdown.selected();
        let Some(id) = self.icon_themes.borrow().get(position as usize).map(|t| t.id.clone()) else { return };
        self.choice.borrow_mut().icons = id;
        self.refresh(false);
    }

    /// A escolha é o tema, a cor e o modo em uso. O destaque não entra: um que não é cor do Papirus (o padrão do
    /// config) deixaria o Aplicar e o Voltar ativos sem nenhuma mudança escolhida. Com o modo, nem a cor: ela vem
    /// do papel de parede.
    fn applied(&self) -> bool {
        let auto = self.current_auto.get();
        let color = if auto { String::new() } else { self.current_color.borrow().clone() };
        (Some(self.wanted.borrow().clone()), color, auto, self.current_icons.borrow().clone()) == self.choice.borrow().key()
    }

    /// Uma mudança de fora (terminal, config editado). Se a escolha era a aplicada, ela acompanha: senão
    /// o Aplicar ligaria e desfaria a mudança. Numa escolha em andamento, só o que a pessoa mexeu fica.
    pub(crate) fn on_theme_changed(&self) {
        // O CSS do tema novo vale depois deste aviso: a cor do texto da janela muda só no próximo quadro
        let strips = self.with(|w, ()| w.sync_strips());
        glib::timeout_add_local_once(Duration::from_millis(300), move || strips(()));
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
            self.load_choice();
        } else {
            // Com uma escolha em andamento, os campos que a pessoa não mexeu acompanham a mudança de fora
            let (current, color, icons) = (self.current.borrow().clone(), self.current_color.borrow().clone(), self.current_icons.borrow().clone());
            let mut choice = self.choice.borrow_mut();
            // A amostra da cor própria escolhida fica, até a pessoa escolher outra
            let picked = choice.is_custom().then(|| choice.custom.clone()).flatten();
            choice.custom = self.current_custom.borrow().clone().or(picked);
            choice.follow(&current, &color, self.current_auto.get());
            choice.follow_icons(&icons);
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
        self.load_choice();
        self.info.set_visible(false);
        self.refresh(true);
    }

    /// Põe a janela de acordo com a escolha.
    pub(crate) fn refresh(&self, scroll: bool) {
        self.updating.set(true);
        let current = self.current.borrow().clone();
        let current_name = sf::family_of(&current).map(|(f, _)| f.to_string()).unwrap_or(current);
        let (custom, is_custom) = {
            let c = self.choice.borrow();
            (c.custom.clone(), c.is_custom())
        };
        let (family_name, other, style, color, theme, accent, has_family, exact, family_accents, auto, wallpaper, default_accent) = {
            let c = self.choice.borrow();
            // A cor das pastas: a do Papirus que vale ou, sem ela (o destaque padrão), a mais perto dele
            let name = c.color_name();
            let default_accent = name.is_empty();
            (
                c.family.as_ref().map(|f| f.name.clone()),
                c.other.clone(),
                c.style,
                if name.is_empty() { sp::nearest_color(&c.accent(), &self.colors) } else { name },
                c.theme(),
                c.accent(),
                c.family.is_some(),
                c.exact(),
                c.family.as_ref().map(|f| f.accents().len()).unwrap_or(0),
                c.auto,
                c.wallpaper,
                default_accent,
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
        // Com o interruptor ligado, as amostras ficam esmaecidas, com a cor do papel marcada (se ela está entre
        // elas); continuam clicáveis: escolher uma desliga o modo
        self.auto_switch.set_active(auto);
        self.auto_switch.set_sensitive(!self.applying.get());
        if auto {
            self.flowbox.add_css_class("theme-auto");
        } else {
            self.flowbox.remove_css_class("theme-auto");
        }
        // A amostra da cor própria, quando a config tem uma
        self.custom_swatch.0.set_visible(custom.is_some());
        if let Some(custom) = &custom {
            *self.custom_color.borrow_mut() = custom.accent.clone();
            self.custom_swatch.0.set_tooltip_text(Some(&custom.caption()));
        }
        // O destaque padrão exato não é uma das amostras: nenhuma marcada (a mais perto só dá a cor das pastas)
        if is_custom {
            self.flowbox.select_child(&self.custom_swatch.0);
        } else {
            match self.swatches.iter().find(|(n, _, _)| *n == color).filter(|_| !default_accent) {
                Some((_, child, _)) => self.flowbox.select_child(child),
                None => self.flowbox.unselect_all(),
            }
        }
        // Os ícones escolhidos na lista; os em uso fora dela (não instalados), nenhum
        let chosen_icons = self.choice.borrow().icons.clone();
        let position = self.icon_themes.borrow().iter().position(|t| t.id == chosen_icons);
        self.icons_dropdown.set_selected(position.map_or(gtk4::INVALID_LIST_POSITION, |p| p as u32));
        self.icons_dropdown.set_sensitive(!self.applying.get() && self.icons_model.n_items() > 0);
        self.sync_strips();
        for (_, _, area) in &self.swatches {
            area.queue_draw();
        }
        self.custom_swatch.1.queue_draw();
        let css_notes = self.css_notes.borrow().join("\n");
        self.css_note.set_text(&css_notes);
        self.css_note.set_visible(!css_notes.is_empty());
        self.auto_dot.queue_draw();
        self.auto_note.set_text(&auto_note(wallpaper));
        self.updating.set(false);
        // O padrão não é uma cor do Papirus: só o hex
        self.color_name.set_text(&if default_accent {
            accent.clone()
        } else if let Some(custom) = custom.as_ref().filter(|_| is_custom) {
            custom.caption()
        } else {
            format!("{color} · {accent}")
        });
        let snap = st::snap_note_with(theme.as_deref().unwrap_or_default(), self.snaps.get());
        self.snap_note.set_text(snap.as_deref().unwrap_or_default());
        self.snap_note.set_visible(snap.is_some());
        let chosen = theme.as_ref().and_then(|t| self.themes.borrow().get(t).cloned());
        let icons = chosen.as_ref().map_or_else(|| "?".to_string(), |t| self.icons_for(t));
        self.preview.set_choice(theme.clone(), style, &accent, &color, &icons);
        // A cor das pastas só com o Papirus: com outros ícones, a nota diz que a cor muda só o destaque
        let papirus_icons = sp::is_papirus(&icons);
        let icons_label = self.icons_display(&icons);
        let mut parts = vec![theme.clone().unwrap_or_else(|| "?".into()), icons_label.clone()];
        if self.papirus.get() && papirus_icons {
            parts.push(fill_one(&tr("%s folders"), &color));
        }
        let folders_note = (self.papirus.get() && !papirus_icons && icons != "?")
            .then(|| fill_one(&tr("The folders of %s don't change color: the color changes the accent only."), &icons_label));
        self.folders_note.set_visible(folders_note.is_some());
        self.folders_note.set_text(folders_note.as_deref().unwrap_or(""));
        self.summary.set_markup(&format!("<b>{}</b> {}", glib::markup_escape_text(&tr("Will apply:")), glib::markup_escape_text(&parts.join(" · "))));
        // No destaque padrão, o aviso de variante fala do hex, não da amostra mais perto
        // A cor padrão e a própria não são amostras: o hex
        let hint_color = if default_accent || is_custom { accent.clone() } else { color.clone() };
        // As pastas só pegam a cor com o Papirus instalado e em uso (a mesma condição do "pastas" do resumo)
        // Com a cor própria, as pastas ficam na cor delas: a dica não diz que elas pegam a exata
        let folders = self.papirus.get() && papirus_icons && !is_custom;
        self.hint.set_text(&self.hint_text(theme.as_deref().unwrap_or(""), &family_name, &hint_color, exact, family_accents, other.is_some(), folders));
        self.update_buttons();
    }

    /// Os ícones que o apply vai gravar: os escolhidos na lista ou, sem mudança nela, os que a troca de tema dá.
    /// Guardados por tema: o refresh roda a cada tecla da busca.
    fn icons_for(&self, chosen: &Theme) -> String {
        if let Some(wanted) = self.choice.borrow().wanted_icons() {
            return st::resolve_icons(&wanted, chosen).unwrap_or(wanted);
        }
        if let Some(icons) = self.icons_cache.borrow().get(&chosen.name) {
            return icons.clone();
        }
        let icons = st::chosen_icons(chosen, &self.config.borrow());
        self.icons_cache.borrow_mut().insert(chosen.name.clone(), icons.clone());
        icons
    }

    /// O aviso de outro desktop vem antes de tudo, somado ao do tema.
    fn hint_text(&self, theme: &str, family: &Option<String>, color: &str, exact: bool, family_accents: usize, other: bool, folders: bool) -> String {
        let foreign = st::foreign_desktop_note_here().map(|note| format!("{note}."));
        let theme_hint = self.theme_hint(theme, family, color, exact, family_accents, other, folders);
        foreign.into_iter().chain(Some(theme_hint)).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
    }

    /// folders: as pastas pegam a cor (ícones Papirus); sem isso, a dica não fala delas.
    fn theme_hint(&self, theme: &str, family: &Option<String>, color: &str, exact: bool, family_accents: usize, other: bool, folders: bool) -> String {
        if let Some(note) = self.note.borrow().as_ref() {
            return fill_one(&tr("%s. Choose a theme to save it in the config."), note);
        }
        if let Some(note) = st::kde_colors_note(theme) {
            return format!("{note}."); // o mais importante: o estilo escolhido não vale para os apps GTK 3
        }
        if let Some(family) = family {
            if !exact && family_accents > 0 {
                let text = if folders {
                    tr("%(family)s has no %(color)s variant: it uses %(theme)s. The bar, the folders and the GTK 4 apps get the exact color.")
                } else {
                    tr("%(family)s has no %(color)s variant: it uses %(theme)s. The bar and the GTK 4 apps get the exact color.")
                };
                return fill(
                    &text,
                    &[("family", family), ("color", color), ("theme", theme)],
                );
            }
        }
        if (family.is_some() && family_accents == 0) || other {
            return if folders {
                tr("This theme keeps its own colors: the accent changes the bar, the folders and the GTK 4 apps.")
            } else {
                tr("This theme keeps its own colors: the accent changes the bar and the GTK 4 apps.")
            };
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
        if *child == self.custom_swatch.0 {
            let hex = self.choice.borrow().custom.as_ref().map(|c| c.accent.clone());
            if let Some(hex) = hex {
                self.choice.borrow_mut().pick_color(&hex);
                self.refresh(false);
            }
            return;
        }
        if let Some((name, _, _)) = self.swatches.iter().find(|(_, c, _)| c == child) {
            self.choice.borrow_mut().pick_color(name); // uma cor escolhida à mão desliga o modo
            self.refresh(false);
        }
    }

    fn on_auto_toggled(&self, active: bool) {
        if self.updating.get() {
            return;
        }
        if !self.applying.get() {
            self.choice.borrow_mut().set_auto(active);
        }
        self.refresh(false);
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
            // Mais de uma nota (um aviso do gtk.css, os ícones): fica até a pessoa fechar. O do gtk.css também fica
            // à vista embaixo das amostras
            self.show_info(gtk4::MessageType::Info, &fill(&tr("%(theme)s applied. %(message)s"), &[("theme", name), ("message", message)]), !message.contains('\n'));
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

/// O pacote que a lista vazia sugere: um tema GTK dos repositórios do Debian, do Ubuntu e do Arch
const SUGGESTED_THEME_PACKAGE: &str = "gnome-themes-extra";

/// O texto da lista de temas sem nenhuma linha: com temas, é a busca que não achou nada; sem nenhum, diz o que
/// instalar. O pacote do app depende do gnome-themes-extra no Arch e o recomenda no Debian e no Ubuntu: a lista
/// fica sem temas com ele removido, ou instalado sem os Recommends
fn empty_text(any_theme: bool) -> String {
    if any_theme {
        tr("No theme found")
    } else {
        fill_one(&tr("No GTK themes are installed. Install one, such as %s."), SUGGESTED_THEME_PACKAGE)
    }
}

/// O que a amostra ao lado do interruptor mostra: a cor do papel ou por que vale o destaque padrão.
fn auto_note(wallpaper: Option<Option<&'static str>>) -> String {
    match wallpaper {
        Some(Some(name)) => format!("{name} · {}", papirus_color(name).unwrap_or_default()),
        Some(None) => fill_one(&tr("The wallpaper has no color: the default accent %s"), config::DEFAULT_ACCENT),
        None => fill_one(&tr("Wallpaper not found or not readable: %s"), &wa::wallpaper().display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_list_says_what_to_install_only_without_any_theme() {
        // Sem catálogo (o i18n sem init), o inglês do código: o mesmo do empty_text do sway-theme-app
        assert_eq!(empty_text(true), "No theme found");
        assert_eq!(empty_text(false), "No GTK themes are installed. Install one, such as gnome-themes-extra.");
    }
}
