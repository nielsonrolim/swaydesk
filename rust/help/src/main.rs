//! Ajuda do Swaydesk (Super+F1 ou Super+/): os atalhos do Sway em uso e um cartão por app do desktop.
//!
//! A aba Atalhos lê o config na hora (swaydesk-help-logic): o principal pelo IPC do Sway e os `include` do
//! disco, então um atalho do config.d/50-local.conf aparece sozinho. A descrição vem do comentário `##`
//! logo acima do bind, traduzida pelo po/; um bind sem ela mostra o comando. A aba Usando o Swaydesk tem um
//! cartão por app, com o atalho de agora (achado pelo comando no config) e o botão que o abre.
//!
//! Digitar busca nas duas abas. Esc limpa a busca e, com ela vazia, fecha. Abrir de novo fecha a janela
//! aberta (o mesmo atalho abre e fecha).
mod cards;

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use swaydesk_common::i18n::{self, ntr, tr};
use swaydesk_common::{config, text::normalize};
use swaydesk_help_logic::{chords, short_command, Chord, Entry, Env, Label};

const APP_ID: &str = "org.swaydesktop.Help"; // igual ao nome do .desktop: é como a dock acha o ícone
const CSS: &str = r#"
.help-top { padding: 10px 14px; }
.help-page { padding: 6px 18px 18px 18px; }
.help-section { font-weight: bold; color: @accent; margin: 14px 0 6px 2px; }
.help-list { border-radius: 10px; border: 1px solid alpha(currentColor, 0.12); }
.help-list row { padding: 7px 12px; }
.help-raw { font-family: monospace; opacity: 0.75; }
.help-plus, .help-or { opacity: 0.55; font-size: smaller; }
.keycap {
  border: 1px solid alpha(currentColor, 0.3);
  border-bottom-width: 2px;
  border-radius: 6px;
  padding: 1px 7px;
  background: alpha(currentColor, 0.07);
  font-weight: bold;
  font-size: smaller;
}
.help-card {
  border-radius: 12px;
  border: 1px solid alpha(currentColor, 0.12);
  background: alpha(currentColor, 0.04);
  padding: 14px;
}
.help-card-title { font-weight: bold; font-size: larger; }
.help-dim { opacity: 0.6; }
.help-empty { opacity: 0.6; margin: 40px; }
.help-off .help-card-title, .help-off image { opacity: 0.55; }
.help-badge {
  border-radius: 999px;
  padding: 0 8px;
  font-size: smaller;
  font-weight: bold;
  background: alpha(currentColor, 0.12);
}
"#;

/// O texto de um chip, na língua do desktop.
fn label_text(label: &Label) -> String {
    match label {
        Label::Text(text) => text.clone(),
        Label::Word(word) => tr(word),
        Label::Fingers(n) => ntr("%d finger", "%d fingers", u64::from(*n)),
        Label::Code(code) => keycode_name(*code),
    }
}

/// A tecla de um bindcode no layout de agora (o 49 é a tecla à esquerda do 1: ' no ABNT2, ` no US).
fn keycode_name(code: u32) -> String {
    let found = gdk::Display::default().and_then(|d| d.map_keycode(code)).and_then(|keys| {
        let (_, key) = keys.into_iter().find(|(k, _)| k.group() == 0 && k.level() == 0)?;
        match key.to_unicode().filter(|c| !c.is_control() && !c.is_whitespace()) {
            Some(c) => Some(c.to_uppercase().collect()),
            None => key.name().map(|n| n.to_string()),
        }
    });
    found.unwrap_or_else(|| format!("#{code}"))
}

/// Os chips de uma combinação: [Super] + [Shift] + [E].
fn chord_box(chord: &Chord) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 3);
    row.set_valign(gtk4::Align::Center);
    for (i, label) in chord.iter().enumerate() {
        if i > 0 {
            let plus = gtk4::Label::new(Some("+"));
            plus.add_css_class("help-plus");
            row.append(&plus);
        }
        let chip = gtk4::Label::new(Some(&label_text(label)));
        chip.add_css_class("keycap");
        row.append(&chip);
    }
    row
}

/// Todas as combinações de uma linha, separadas por "or".
fn chords_box(list: &[Chord]) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_halign(gtk4::Align::End);
    for (i, chord) in list.iter().enumerate() {
        if i > 0 {
            let or = gtk4::Label::new(Some(&tr("or")));
            or.add_css_class("help-or");
            row.append(&or);
        }
        row.append(&chord_box(chord));
    }
    row
}

fn chords_text(list: &[Chord]) -> String {
    list.iter().flatten().map(label_text).collect::<Vec<_>>().join(" ")
}

/// Uma linha que a busca mostra ou esconde.
struct Searchable {
    widget: gtk4::Widget,
    /// O texto da busca, já normalizado (sem acentos, minúsculo).
    haystack: String,
    group: usize,
}

/// Um grupo: a seção dos atalhos (o título e a lista) ou a grade dos cartões.
struct Group {
    widgets: Vec<gtk4::Widget>,
}

struct Page {
    rows: Vec<Searchable>,
    groups: Vec<Group>,
    empty: gtk4::Label,
}

impl Page {
    /// Mostra o que tem todas as palavras da busca; um grupo sem nada visível some inteiro.
    fn filter(&self, query: &str) {
        let words: Vec<String> = normalize(query).split_whitespace().map(str::to_string).collect();
        let mut visible = vec![false; self.groups.len()];
        for row in &self.rows {
            let show = words.iter().all(|w| row.haystack.contains(w.as_str()));
            row.widget.set_visible(show);
            visible[row.group] |= show;
        }
        for (group, shown) in self.groups.iter().zip(&visible) {
            for widget in &group.widgets {
                widget.set_visible(*shown);
            }
        }
        self.empty.set_visible(!visible.iter().any(|v| *v));
    }
}

fn empty_label() -> gtk4::Label {
    let empty = gtk4::Label::new(Some(&tr("Nothing matches the search")));
    empty.add_css_class("help-empty");
    empty.set_visible(false);
    empty
}

fn scrolled(child: &impl IsA<gtk4::Widget>) -> gtk4::ScrolledWindow {
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(child));
    scroll
}

/// A seção de cada linha: a do `###`, a do modo do Sway, ou "Other" no fim.
fn section_of(entry: &Entry) -> Option<String> {
    entry.section.as_deref().map(tr).or_else(|| entry.mode.as_ref().map(|m| tr("Mode “%s”").replace("%s", m)))
}

fn shortcuts_page(entries: &[Entry]) -> (gtk4::ScrolledWindow, Page) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.add_css_class("help-page");
    // A ordem das seções é a do config; as linhas sem seção ficam no fim
    let mut order: Vec<Option<String>> = Vec::new();
    let mut by_section: HashMap<Option<String>, Vec<&Entry>> = HashMap::new();
    for entry in entries {
        let key = section_of(entry);
        if !by_section.contains_key(&key) {
            order.push(key.clone());
        }
        by_section.entry(key).or_default().push(entry);
    }
    order.sort_by_key(Option::is_none);
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    for key in order {
        let title = gtk4::Label::new(Some(&key.clone().unwrap_or_else(|| tr("Other"))));
        title.add_css_class("help-section");
        title.set_xalign(0.0);
        let list = gtk4::ListBox::new();
        list.set_selection_mode(gtk4::SelectionMode::None);
        list.add_css_class("help-list");
        for entry in &by_section[&key] {
            let row = shortcut_row(entry);
            list.append(&row);
            let section = key.clone().unwrap_or_default();
            let commands: Vec<String> = entry.binds.iter().map(|b| short_command(&b.command)).collect();
            let haystack = normalize(&format!(
                "{} {} {} {}",
                entry.description.as_deref().map(tr).unwrap_or_default(),
                section,
                commands.join(" "),
                chords_text(&chords(entry))
            ));
            rows.push(Searchable { widget: row.upcast(), haystack, group: groups.len() });
        }
        content.append(&title);
        content.append(&list);
        groups.push(Group { widgets: vec![title.upcast(), list.upcast()] });
    }
    let empty = empty_label();
    if entries.is_empty() {
        empty.set_label(&tr("No shortcuts found in the Sway config"));
        empty.set_visible(true);
    }
    content.append(&empty);
    (scrolled(&content), Page { rows, groups, empty })
}

fn shortcut_row(entry: &Entry) -> gtk4::ListBoxRow {
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let text = match &entry.description {
        Some(desc) => gtk4::Label::new(Some(&tr(desc))),
        None => {
            let raw = gtk4::Label::new(Some(&short_command(&entry.binds[0].command)));
            raw.add_css_class("help-raw");
            raw
        }
    };
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_wrap(true);
    text.set_wrap_mode(pango::WrapMode::WordChar);
    line.append(&text);
    if entry.binds.iter().any(|b| b.locked) {
        let lock = gtk4::Image::from_icon_name("system-lock-screen-symbolic");
        lock.set_tooltip_text(Some(&tr("Also works on the lock screen")));
        lock.add_css_class("help-dim");
        line.append(&lock);
    }
    line.append(&chords_box(&chords(entry)));
    let row = gtk4::ListBoxRow::new();
    row.set_activatable(false);
    row.set_child(Some(&line));
    row
}

fn cards_page(
    entries: &[Entry],
    window: &gtk4::ApplicationWindow,
    config: &config::Config,
) -> (gtk4::ScrolledWindow, Page) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.add_css_class("help-page");
    let grid = gtk4::FlowBox::new();
    grid.set_selection_mode(gtk4::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(3);
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_valign(gtk4::Align::Start);
    let mut rows = Vec::new();
    for card in cards::cards() {
        if !card.available() {
            continue;
        }
        let widget = card.widget(entries, window, config);
        let child = gtk4::FlowBoxChild::new();
        child.set_focusable(false);
        child.set_child(Some(&widget));
        grid.append(&child);
        let haystack = normalize(&format!("{} {}", card.name, card.text));
        rows.push(Searchable { widget: child.upcast(), haystack, group: 0 });
    }
    content.append(&grid);
    let empty = empty_label();
    content.append(&empty);
    (scrolled(&content), Page { rows, groups: vec![Group { widgets: vec![grid.upcast()] }], empty })
}

fn build(app: &gtk4::Application) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::new(app);
    window.set_title(Some(&tr("Swaydesk Help")));
    window.set_icon_name(Some("help-browser"));
    window.set_default_size(960, 680);

    let provider = gtk4::CssProvider::new();
    // As cores do destaque e as fontes do desktop (swaydesk_ui::theme_css), como os outros apps
    provider.load_from_string(&format!("{}{CSS}", swaydesk_ui::theme_css()));
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let entries = swaydesk_help_logic::load(&Env::from_process());
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    let (shortcuts, shortcuts_filter) = shortcuts_page(&entries);
    let (using, using_filter) = cards_page(&entries, &window, &config::load());
    stack.add_titled(&shortcuts, Some("shortcuts"), &tr("Shortcuts"));
    stack.add_titled(&using, Some("using"), &tr("Using Swaydesk"));

    let switcher = gtk4::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_hexpand(true);
    switcher.set_halign(gtk4::Align::Start);
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some(&tr("Search")));
    search.set_width_chars(28);
    // Digitar em qualquer lugar da janela vai para a busca
    search.set_key_capture_widget(Some(&window));
    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    top.add_css_class("help-top");
    top.append(&switcher);
    top.append(&search);

    let pages = Rc::new([shortcuts_filter, using_filter]);
    search.connect_search_changed({
        let pages = pages.clone();
        move |entry| {
            for page in pages.iter() {
                page.filter(&entry.text());
            }
        }
    });

    // Esc: com texto, limpa a busca; vazia, fecha. Ctrl+Tab e Ctrl+PgUp/PgDn trocam de aba (o foco fica na
    // busca, e o Tab sozinho só anda pelos controles). Na fase de captura, antes do SearchEntry, que só
    // limparia a busca e pararia ali
    let keys = gtk4::EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let window = window.downgrade();
        let search = search.downgrade();
        let stack = stack.downgrade();
        move |_, key, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let switch_tab =
                ctrl && matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab | gdk::Key::Page_Up | gdk::Key::Page_Down);
            if switch_tab {
                if let Some(stack) = stack.upgrade() {
                    let next =
                        if stack.visible_child_name().as_deref() == Some("shortcuts") { "using" } else { "shortcuts" };
                    stack.set_visible_child_name(next);
                }
                return glib::Propagation::Stop;
            }
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            match (search.upgrade(), window.upgrade()) {
                (Some(search), _) if !search.text().is_empty() => search.set_text(""),
                (_, Some(window)) => window.close(),
                _ => {}
            }
            glib::Propagation::Stop
        }
    });
    window.add_controller(keys);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    body.append(&top);
    body.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    body.append(&stack);
    window.set_child(Some(&body));
    window.present();
    search.grab_focus();
    window
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o sway-help em Python confere que o binário carrega
    }
    swaydesk_ui::use_cairo_renderer();
    // Sem isso, o app_id no Sway é o nome do programa, e a dock não acha o .desktop nem o ícone
    glib::set_prgname(Some(APP_ID));
    i18n::init(&config::load());
    swaydesk_common::theme_resources::register_theme_resources();
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::default());
    let window: Rc<RefCell<Option<gtk4::ApplicationWindow>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        // O mesmo atalho abre e fecha: chamar de novo com a janela aberta a fecha
        let existing = window.borrow_mut().take();
        match existing {
            Some(w) if w.is_visible() => w.close(),
            _ => *window.borrow_mut() = Some(build(app)),
        }
    });
    let code = app.run_with_args(&std::env::args().take(1).collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
