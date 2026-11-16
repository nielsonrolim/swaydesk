//! Ajuda do Swaydesk (Super+F1 ou Super+/): os atalhos do Sway em uso e um cartão por app do desktop.
//!
//! A aba Atalhos lê o config na hora (swaydesk-help-logic): o principal pelo IPC do Sway e os `include` do
//! disco, então um atalho do config.d/50-local.conf aparece sozinho. A descrição vem do comentário `##`
//! logo acima do bind, traduzida pelo po/; um bind sem ela mostra o comando. A aba Usando o Swaydesk tem um
//! cartão por app, com o atalho de agora (achado pelo comando no config) e o botão que o abre.
//!
//! Digitar busca nas duas abas. Esc limpa a busca e, com ela vazia, fecha. Abrir de novo fecha a janela
//! aberta (o mesmo atalho abre e fecha).
//!
//! Com mais de um perfil de atalhos instalado (i3, Hyprland...), um seletor no alto troca o perfil: grava
//! `shortcuts` no config.toml, gera o config.d/00-keys.conf (como o sway-desktop-config generate), recarrega
//! o Sway e lê de novo os atalhos, que já são os do perfil novo.
mod cards;

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use swaydesk_common::i18n::{self, ntr, ptr, tr};
use swaydesk_common::{config, text::normalize};
use swaydesk_help_logic::profiles::{self, Profile};
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
.help-notice.help-banner { margin: 0 14px 10px 14px; }
.help-notice {
  border-radius: 10px;
  border: 1px solid alpha(@accent, 0.5);
  background: alpha(@accent, 0.1);
  padding: 10px 12px;
  margin: 12px 0 4px 0;
}
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

fn shortcuts_page(entries: &[Entry], notice: Option<String>) -> (gtk4::ScrolledWindow, Page) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.add_css_class("help-page");
    // O aviso do perfil em uso (o dwm: o Mod é Alt), antes dos atalhos
    if let Some(notice) = notice {
        let label = gtk4::Label::new(Some(&notice));
        label.add_css_class("help-notice");
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        content.append(&label);
    }
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
    content.append(&local_footer());
    (scrolled(&content), Page { rows, groups, empty })
}

/// O fim da aba Atalhos: onde mudar um atalho (o 50-local.conf, que vale depois do perfil e aparece aqui) e um
/// botão que o abre no editor padrão. Sem o arquivo, só o texto.
fn local_footer() -> gtk4::Box {
    let file = swaydesk_help_logic::profiles::local_shortcuts_file(&Env::from_process());
    let shown = file
        .strip_prefix(swaydesk_common::config::home())
        .map(|rel| format!("~/{}", rel.display()))
        .unwrap_or_else(|_| file.display().to_string());
    let footer = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    footer.set_margin_top(18);
    let text = gtk4::Label::new(Some(
        &tr("To change a shortcut, edit %s: it's read after the profile, and its shortcuts show up here.")
            .replace("%s", &shown),
    ));
    text.add_css_class("help-dim");
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_wrap(true);
    text.set_wrap_mode(pango::WrapMode::WordChar);
    footer.append(&text);
    if file.is_file() {
        // "Open" de botão (o verbo): o "Open" sem contexto é o adjetivo da rede ("Aberta")
        let open = gtk4::Button::with_label(&ptr("button", "Open"));
        open.connect_clicked(move |button| {
            let window = button.root().and_downcast::<gtk4::Window>();
            open_text_file(&file, window.as_ref());
        });
        footer.append(&open);
    }
    footer
}

/// Abre um arquivo de texto no editor padrão do sistema (o app do text/plain), direto, quando ele é gráfico: o
/// portal perguntaria qual app usar toda vez. Sem um editor gráfico (decisão do usuário), num editor do terminal:
/// o terminal do Swaydesk com o $VISUAL, o $EDITOR, o nano, o vim ou o vi. Só se o terminal não abrir, o portal
/// (o GtkFileLauncher). Lê as associações do sistema, sem mudar nenhuma.
fn open_text_file(file: &std::path::Path, window: Option<&gtk4::Window>) {
    let gfile = gio::File::for_path(file);
    if let Some(app) = gio::AppInfo::default_for_type("text/plain", false) {
        let terminal = app.downcast_ref::<gio::DesktopAppInfo>().is_some_and(|d| d.boolean("Terminal"));
        if !terminal {
            let context = window.map(|w| gtk4::prelude::WidgetExt::display(w).app_launch_context());
            if app.launch(&[gfile.clone()], context.as_ref()).is_ok() {
                return;
            }
        }
    }
    let command = swaydesk_help_logic::profiles::terminal_editor_command(
        file,
        &tr("No text editor found: install gnome-text-editor or nano. Press Enter to close."),
    );
    // Desligado da Ajuda (swaydesk_ui::cmd): fechar a Ajuda não fecha o editor, e não fica processo zumbi
    if swaydesk_ui::cmd::spawn_detached(&command[0], &command[1..]).is_err() {
        gtk4::FileLauncher::new(Some(&gfile)).launch(window, None::<&gio::Cancellable>, |_| {});
    }
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

/// As duas abas, com os atalhos lidos agora. Na troca de perfil, as abas velhas saem e estas entram no lugar,
/// na mesma aba que estava à vista.
fn fill(stack: &gtk4::Stack, window: &gtk4::ApplicationWindow) -> [Page; 2] {
    let visible = stack.visible_child_name();
    for name in ["shortcuts", "using"] {
        if let Some(child) = stack.child_by_name(name) {
            stack.remove(&child);
        }
    }
    let entries = swaydesk_help_logic::load(&Env::from_process());
    let (shortcuts, shortcuts_filter) = shortcuts_page(&entries, profile_notice(&current_profile()));
    let (using, using_filter) = cards_page(&entries, window, &config::load());
    stack.add_titled(&shortcuts, Some("shortcuts"), &tr("Shortcuts"));
    stack.add_titled(&using, Some("using"), &tr("Using Swaydesk"));
    if let Some(name) = visible {
        stack.set_visible_child_name(&name);
    }
    [shortcuts_filter, using_filter]
}

/// Os perfis instalados, para o seletor. Vazio ou um só: sem seletor.
fn installed_profiles() -> Vec<Profile> {
    profiles::available(config::SHORTCUT_PROFILES, |id| config::shortcut_keys_file(id).is_some())
}

/// Troca o perfil: o config.toml (só a linha `shortcuts`), o config.d/00-keys.conf (o mesmo que o
/// `sway-desktop-config keys` grava, sem o resto do tema) e o reload do Sway. Fora da thread da janela: o
/// reload pode levar uns segundos.
fn apply_profile(id: &str) -> Result<(), String> {
    use swaydesk_theme::desktop;
    swaydesk_theme::themes::set_config_values(&config::path(), &[("shortcuts", id.to_string())])
        .map_err(|e| e.to_string())?;
    let keys = desktop::sway_keys(&config::load(), &config::keys_dir());
    desktop::write_file(&desktop::config_home().join("sway/config.d/00-keys.conf"), &keys).map_err(|e| e.to_string())?;
    if !swaydesk_theme::desktop::reload_sway() {
        return Err(tr("Sway didn't reload"));
    }
    Ok(())
}

/// O aviso de um perfil que muda mais que as teclas: o dwm usa Alt como Mod (decisão do usuário), e alguns
/// atalhos dos apps com Alt deixam de chegar a eles. O mesmo texto abre o keys/dwm.conf (builder5).
fn profile_notice(id: &str) -> Option<String> {
    match id {
        "dwm" => Some(tr("dwm profile: the modifier is Alt, as in dwm, not Super. Some app shortcuts with Alt (like Alt+F, which opens the File menu) no longer reach the apps. Two shortcuts change only here: the window cards (Alt+Tab and Alt+Shift+Tab in the other profiles) move to Super+Tab and Super+Shift+Tab, because Alt+Tab goes back to the last workspace; and the launcher (Alt+Space) moves to Super+Space, because Alt+Space switches the window layout. The Swaydesk actions that dwm doesn't have (notifications, clipboard, window search, capture and recording) stay on Super. Floating windows with the mouse: Alt+drag. The keyboard layout: Super+Ctrl+Space.")),
        _ => None,
    }
}

/// O perfil em uso, pelo config.toml de agora (o swaydesk se faltar ou for inválido).
fn current_profile() -> String {
    config::load().shortcuts
}

/// A faixa sob a barra de cima, dentro da janela: o erro de uma troca ou a confirmação do dwm (o aviso e
/// Cancelar/Trocar). Dentro da janela, o texto quebra em vez de alargá-la, e o Sway não a põe num canto.
struct Banner {
    revealer: gtk4::Revealer,
    title: gtk4::Label,
    text: gtk4::Label,
    buttons: gtk4::Box,
    cancel: gtk4::Button,
    confirm: gtk4::Button,
}

impl Banner {
    fn new() -> Self {
        let title = gtk4::Label::new(None);
        title.add_css_class("help-card-title");
        title.set_xalign(0.0);
        let text = gtk4::Label::new(None);
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_wrap_mode(pango::WrapMode::WordChar);
        let words = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        words.set_hexpand(true);
        words.append(&title);
        words.append(&text);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        let confirm = gtk4::Button::with_label(&tr("Switch"));
        confirm.add_css_class("suggested-action");
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_valign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&confirm);
        let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
        inner.add_css_class("help-notice");
        inner.add_css_class("help-banner");
        inner.append(&words);
        inner.append(&buttons);
        let revealer = gtk4::Revealer::new();
        revealer.set_child(Some(&inner));
        Banner { revealer, title, text, buttons, cancel, confirm }
    }

    fn error(&self, message: &str) {
        self.title.set_visible(false);
        self.text.set_label(message);
        self.buttons.set_visible(false);
        self.revealer.set_reveal_child(true);
    }

    fn confirm(&self, title: &str, notice: &str) {
        self.title.set_label(title);
        self.title.set_visible(true);
        self.text.set_label(notice);
        self.buttons.set_visible(true);
        self.revealer.set_reveal_child(true);
        self.confirm.grab_focus();
    }

    fn hide(&self) {
        self.revealer.set_reveal_child(false);
    }
}

/// O seletor do perfil, com a faixa que vai sob a barra de cima: o nome de cada perfil instalado, com o de agora
/// escolhido. Na troca, aplica e, se deu certo, refaz as abas; senão mostra o erro na faixa. Um perfil com aviso
/// (o dwm) pede confirmação antes, na faixa; Cancelar volta o seletor ao perfil em uso.
fn profile_selector(stack: &gtk4::Stack, window: &gtk4::ApplicationWindow, pages: &Rc<RefCell<[Page; 2]>>,
                    search: &gtk4::SearchEntry) -> Option<(gtk4::DropDown, gtk4::Revealer)> {
    let installed = installed_profiles();
    if installed.len() < 2 {
        return None;
    }
    let current = current_profile();
    let names: Vec<&str> = installed.iter().map(|p| p.name.as_str()).collect();
    let dropdown = gtk4::DropDown::from_strings(&names);
    dropdown.set_tooltip_text(Some(&tr("Shortcut profile")));
    let at = installed.iter().position(|p| p.id == current).unwrap_or(0);
    dropdown.set_selected(at as u32);
    let banner = Rc::new(Banner::new());
    let applied = Rc::new(RefCell::new(installed[at].id.clone()));
    let ids: Vec<String> = installed.iter().map(|p| p.id.clone()).collect();

    // Aplica um perfil (fora da thread da janela) e refaz as abas
    let switch: Rc<dyn Fn(String)> = Rc::new({
        let (dropdown, stack, window) = (dropdown.downgrade(), stack.downgrade(), window.downgrade());
        let (pages, search, banner, applied) = (pages.clone(), search.downgrade(), banner.clone(), applied.clone());
        let ids = ids.clone();
        move |id: String| {
            let Some(dropdown) = dropdown.upgrade() else { return };
            dropdown.set_sensitive(false);
            banner.hide();
            let (stack, window, pages, search, banner, applied, ids) =
                (stack.clone(), window.clone(), pages.clone(), search.clone(), banner.clone(), applied.clone(), ids.clone());
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking({
                    let id = id.clone();
                    move || apply_profile(&id)
                })
                .await
                .unwrap_or_else(|_| Err(tr("Sway didn't reload")));
                dropdown.set_sensitive(true);
                match result {
                    Ok(()) => {
                        *applied.borrow_mut() = id;
                        if let (Some(stack), Some(window)) = (stack.upgrade(), window.upgrade()) {
                            *pages.borrow_mut() = fill(&stack, &window);
                            if let Some(search) = search.upgrade() {
                                for page in pages.borrow().iter() {
                                    page.filter(&search.text());
                                }
                            }
                        }
                    }
                    Err(err) => {
                        banner.error(&tr("Couldn't switch the shortcut profile: %s").replace("%s", &err));
                        // O config.toml pode já ter o perfil novo (a falha veio depois de gravá-lo): o seletor
                        // volta a mostrar o que o arquivo diz. Um perfil sem arquivo instalado não está no
                        // seletor: o applied fica com ele mesmo assim, e qualquer escolha grava e aplica de novo.
                        // O set_selected não aplica nada: o handler ignora o perfil igual ao aplicado
                        let now = current_profile();
                        let at = ids.iter().position(|p| *p == now);
                        *applied.borrow_mut() = now;
                        if let Some(at) = at {
                            dropdown.set_selected(at as u32);
                        }
                    }
                }
            });
        }
    });

    // A confirmação pendente: o perfil escolhido e a posição do aplicado, para o Cancelar. Sem posição (um
    // aplicado sem arquivo instalado, depois de uma falha), o Cancelar só esconde a faixa: voltar o seletor a
    // outro perfil o aplicaria sem a pessoa pedir
    let pending: Rc<RefCell<Option<(String, Option<u32>)>>> = Rc::new(RefCell::new(None));
    banner.cancel.connect_clicked({
        let (dropdown, banner, pending) = (dropdown.downgrade(), banner.clone(), pending.clone());
        move |_| {
            banner.hide();
            let taken = pending.borrow_mut().take();
            if let (Some((_, Some(back))), Some(dropdown)) = (taken, dropdown.upgrade()) {
                dropdown.set_selected(back); // o mesmo que o aplicado: o handler não faz nada
            }
        }
    });
    banner.confirm.connect_clicked({
        let (switch, pending) = (switch.clone(), pending.clone());
        move |_| {
            let taken = pending.borrow_mut().take();
            if let Some((id, _)) = taken {
                switch(id);
            }
        }
    });

    dropdown.connect_selected_notify({
        let banner = banner.clone();
        move |dropdown| {
            let Some(profile) = installed.get(dropdown.selected() as usize) else { return };
            if *applied.borrow() == profile.id {
                pending.borrow_mut().take();
                return; // also the way back after a Cancel
            }
            let id = profile.id.clone();
            let Some(notice) = profile_notice(&id) else {
                pending.borrow_mut().take();
                switch(id);
                return;
            };
            let back = ids.iter().position(|p| *p == *applied.borrow()).map(|at| at as u32);
            *pending.borrow_mut() = Some((id, back));
            banner.confirm(&tr("Switch to the %s profile?").replace("%s", &profile.name), &notice);
        }
    });
    Some((dropdown, banner.revealer.clone()))
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

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    let pages = Rc::new(RefCell::new(fill(&stack, &window)));

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
    let selector = profile_selector(&stack, &window, &pages, &search);
    if let Some((dropdown, _)) = &selector {
        top.append(dropdown);
    }
    top.append(&search);

    search.connect_search_changed({
        let pages = pages.clone();
        move |entry| {
            for page in pages.borrow().iter() {
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
    if let Some((_, banner)) = &selector {
        body.append(banner);
    }
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
