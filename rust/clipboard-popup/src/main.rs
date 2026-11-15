//! Popup de clipboard para o clique no ícone da Waybar e para o Super+V: o porte de
//! `home/.local/bin/waybar-clipboard-popup`, que passa a vez a este binário (sway_rust). O histórico do
//! cliphist, com busca no texto completo, miniaturas de imagens, itens fixados independentes e limpeza do
//! histórico. A lógica é a do swaydesk-clipboard, nos mesmos arquivos do Python.
//!
//! O teclado fica sempre na busca: setas ou Tab movem a seleção e Enter copia o item selecionado. Ler
//! cada item do histórico é um processo por item: isso roda em outra linha, e o resultado chega à janela
//! em lotes. A lista rola dentro de uma altura fixa (redimensionar faria o Sway tirar o foco).
mod index;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, glib, pango};
use index::{index_item, xdg, Indexed, Task};
use swaydesk_clipboard::{self as wc, read_content, ClipboardError, Confirm, Entry, Pin, PinStore, Tools};
use swaydesk_common::i18n::{ntr, ptr, tr};
use swaydesk_ui::list::{empty_label, link_button, scroller, set_status, CSS_DEV, CSS_LIST, SCROLL_HEIGHT};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-clipboard-popup";
const ROW_SIZE: i32 = 32;
const CARD_WIDTH: i32 = 360;
const INDEX_BATCH: usize = 25;
/// Linhas montadas antes de a janela aparecer; as outras entram depois, aos poucos
const FIRST_ROWS: usize = 40;
const SCROLL_RESTORE: Duration = Duration::from_millis(500); // prazo para a lista nova ter tamanho (restore_scroll)
const MORE_ROWS: usize = 60;
const INDEX_BATCH_TIME: Duration = Duration::from_millis(150);
const UTF8_TEXT: &str = "text/plain;charset=utf-8";

// Material da Nerd Font, como nos popups de rede e de Bluetooth
const ICON_PIN: &str = "\u{f0931}";
const ICON_PIN_ON: &str = "\u{f0403}";
const ICON_TRASH: &str = "\u{f09e7}";
const ICON_TEXT: &str = "\u{f09ee}";
const ICON_IMAGE: &str = "\u{f0976}";
const ICON_BINARY: &str = "\u{f0224}";

const CSS: &str = r#"
.box { background: #38383d; border-radius: 10px; padding: 6px; }
.clip-list { background: transparent; }
.clip-list row {
  background: transparent; border: none; box-shadow: none; outline-style: none;
  border-radius: 8px; padding: 7px 10px; margin-bottom: 2px;
}
.clip-list row:last-child { margin-bottom: 0; }
.clip-list row:hover { background: #4c4c50; }
.clip-list row:selected { background: @accent; color: @on_accent; }
.clip-list row:selected label { color: @on_accent; }
.clip-list row:selected:hover { background: @accent_hover; }
.clip-list row:selected .dev-sub { color: @accent_soft; }
.clip-list button.off { padding: 2px 5px; color: #8a8a8f; opacity: 0; }
.clip-list row:hover button.off, .clip-list row:selected button.off,
.clip-list button.off.on { opacity: 1; }
.clip-list button.off:hover { background: rgba(255, 255, 255, 0.12); color: #ffffff; }
.clip-list button.off.on { color: @accent; }
.clip-list button.trash:hover { color: #fc4138; }
.clip-list row:selected button.off, .clip-list row:selected button.off label { color: @on_accent; }
.clip-list row:selected button.off:hover { background: alpha(@on_accent, 0.15); }
entry {
  background: #38383d; color: #dadada; border: none; box-shadow: none; outline: none;
  border-radius: 8px; font-size: 13px; min-height: 0; padding: 6px 10px;
}
entry:focus-within { box-shadow: inset 0 0 0 1px @accent; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
button.link.danger, button.link.danger:hover { color: #fc4138; }
"#;

/// A prévia numa linha: os espaços do Python (`str.split`), e "(vazio)" sem nada.
fn clean_preview(preview: &str) -> String {
    let text = wc::display_preview(preview);
    let joined = text.split(wc::content::py_isspace).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    if joined.is_empty() { "(vazio)".into() } else { joined }
}

fn make_title(kind: &str, source: &str) -> String {
    match kind {
        wc::IMAGE => tr("Image"),
        wc::BINARY => tr("Binary content"),
        _ => clean_preview(source),
    }
}

fn make_sub(kind: &str, mime: Option<&str>) -> String {
    match kind {
        wc::IMAGE => {
            let format = mime.unwrap_or("").rsplit('/').next().unwrap_or("").to_uppercase();
            if format.is_empty() { tr("Unknown format") } else { format }
        }
        wc::BINARY => mime.filter(|m| !m.is_empty()).map(String::from).unwrap_or_else(|| tr("Unknown format")),
        _ => tr("Text"),
    }
}

/// Um texto em UTF-16 tem a prévia ilegível; o texto completo, quando chega, é melhor.
fn title_source(preview: &str, full_text: Option<&str>) -> String {
    match full_text {
        Some(text) if !text.is_empty() && preview.contains('\0') => text.chars().take(200).collect(),
        _ => preview.to_string(),
    }
}

type Key = (String, String);

fn key(entry: &Entry) -> Key {
    (entry.id.clone(), entry.preview.clone())
}

/// Uma linha de Fixados ou de Recentes; guarda os dados para copiar e filtrar.
struct ClipRow {
    row: gtk4::ListBoxRow,
    entry: Option<Entry>,
    pin: Option<Pin>,
    kind: RefCell<String>,
    preview: String,
    /// O rótulo do binário, só para mostrar (o MIME do wl-copy fica como está)
    label: RefCell<Option<String>>,
    full_text: RefCell<Option<String>>,
    search: RefCell<Option<String>>,
    utf8: RefCell<Option<String>>,
    /// O conteúdo já foi lido pela outra linha; antes disso, a cópia lê na hora
    content_read: Cell<bool>,
    thumb_box: gtk4::CenterBox,
    glyph: gtk4::Label,
    title: gtk4::Label,
    sub: gtk4::Label,
    pin_button: gtk4::Button,
    trash_button: gtk4::Button,
}

impl ClipRow {
    fn new(entry: Option<Entry>, pin: Option<Pin>, item: Option<&Indexed>) -> Rc<ClipRow> {
        let (mut kind, mut mime, preview) = match (&pin, &entry) {
            (Some(p), _) => (p.kind.clone(), p.mime.clone(), p.preview.clone()),
            (None, Some(e)) => (e.kind.clone(), e.mime.clone(), e.preview.clone()),
            _ => unreachable!(),
        };
        let full_text = item.and_then(|i| i.text.clone());
        let binary = item.and_then(|i| i.binary.clone());
        let mut label_text = None;
        if let Some(b) = &binary {
            kind = wc::BINARY.into();
            mime = None;
            label_text = Some(b.clone()).filter(|b| !b.is_empty());
        }
        let row = gtk4::ListBoxRow::new();
        row.set_activatable(true);
        row.set_focusable(false); // o foco fica na busca
        row.set_tooltip_text(Some(&tr("Click to copy")));
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.set_child(Some(&content));
        let thumb_box = gtk4::CenterBox::new();
        thumb_box.set_size_request(ROW_SIZE, ROW_SIZE);
        thumb_box.set_valign(gtk4::Align::Center);
        let glyph = label(
            match kind.as_str() {
                wc::IMAGE => ICON_IMAGE,
                wc::BINARY => ICON_BINARY,
                _ => ICON_TEXT,
            },
            &["glyph"],
            0.5,
        );
        thumb_box.set_center_widget(Some(&glyph));
        content.append(&thumb_box);
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
        col.set_hexpand(true);
        let title = label(&make_title(&kind, &title_source(&preview, full_text.as_deref())), &["dev-name"], 0.0);
        title.set_ellipsize(pango::EllipsizeMode::End);
        col.append(&title);
        let sub = label(&make_sub(&kind, label_text.as_deref().or(mime.as_deref())), &["dev-sub"], 0.0);
        sub.set_ellipsize(pango::EllipsizeMode::End);
        col.append(&sub);
        content.append(&col);
        let pinned = pin.is_some();
        let pin_button = gtk4::Button::with_label(if pinned { ICON_PIN_ON } else { ICON_PIN });
        let trash_button = gtk4::Button::with_label(ICON_TRASH);
        for (b, extra) in [(&pin_button, if pinned { Some("on") } else { None }), (&trash_button, Some("trash"))] {
            b.set_valign(gtk4::Align::Center);
            b.set_focusable(false);
            b.add_css_class("off");
            if let Some(extra) = extra {
                b.add_css_class(extra);
            }
            content.append(b);
        }
        pin_button.set_tooltip_text(Some(&if pinned { tr("Unpin") } else { tr("Pin") }));
        trash_button.set_tooltip_text(Some(&if pinned { tr("Remove from pinned") } else { tr("Delete from history") }));
        Rc::new(ClipRow {
            row,
            entry,
            pin,
            kind: RefCell::new(kind),
            preview,
            label: RefCell::new(label_text),
            content_read: Cell::new(full_text.is_some() || binary.is_some()),
            full_text: RefCell::new(full_text),
            search: RefCell::new(item.and_then(|i| i.search.clone())),
            utf8: RefCell::new(item.and_then(|i| i.utf8.clone())),
            thumb_box,
            glyph,
            title,
            sub,
            pin_button,
            trash_button,
        })
    }

    fn is_image(&self) -> bool {
        *self.kind.borrow() == wc::IMAGE
    }

    fn set_thumb(&self, png: &[u8]) {
        if !self.is_image() {
            return;
        }
        let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(png.to_vec())) else { return };
        let (w, h) = wc::scale_box(pixbuf.width(), pixbuf.height(), ROW_SIZE);
        let scaled = if (w, h) != (pixbuf.width(), pixbuf.height()) { pixbuf.scale_simple(w, h, gdk_pixbuf::InterpType::Bilinear) } else { Some(pixbuf) };
        if let Some(scaled) = scaled {
            self.set_texture_sized(&gdk::Texture::for_pixbuf(&scaled), w, h);
        }
    }

    fn set_texture_sized(&self, texture: &gdk::Texture, w: i32, h: i32) {
        // Um Image no tamanho da caixa: o Picture recalcula a largura pela altura da linha e a alarga
        let image = gtk4::Image::from_paintable(Some(texture));
        image.set_pixel_size(w.max(h));
        image.set_halign(gtk4::Align::Center);
        image.set_valign(gtk4::Align::Center);
        self.thumb_box.set_center_widget(Some(&image));
    }

    fn fields(&self) -> Vec<String> {
        let kind = self.kind.borrow().clone();
        let label = self.label.borrow().clone();
        let full = self.full_text.borrow().clone();
        match (&self.pin, &self.entry) {
            (Some(p), _) => {
                let item = Entry { id: p.source_id.clone(), preview: p.preview.clone(), kind, mime: label.or(p.mime.clone()) };
                let mut f = wc::search_fields(&item, full.as_deref());
                f.push("fixado".into());
                f
            }
            (None, Some(e)) => {
                let item = Entry { kind, mime: label.or(e.mime.clone()), ..e.clone() };
                wc::search_fields(&item, full.as_deref())
            }
            _ => Vec::new(),
        }
    }

    fn search_text(&self) -> String {
        if self.search.borrow().is_none() {
            let text = wc::haystack(&self.fields());
            self.search.replace(Some(text));
        }
        self.search.borrow().clone().unwrap_or_default()
    }

    fn set_indexed(&self, item: &Indexed) {
        // O fixado refaz a busca pelos campos dele, que incluem "fixado"
        let search = if self.pin.is_none() { item.search.clone() } else { None };
        if item.text.is_some() || item.binary.is_some() {
            self.content_read.set(true);
        }
        if let Some(binary) = &item.binary {
            if *self.kind.borrow() == wc::TEXT {
                // A prévia parecia texto (cliphist 0.5), mas o conteúdo é binário
                self.kind.replace(wc::BINARY.into());
                self.label.replace(Some(binary.clone()).filter(|b| !b.is_empty()));
                self.full_text.replace(None);
                self.glyph.set_text(ICON_BINARY);
                self.title.set_text(&make_title(wc::BINARY, &self.preview));
                self.sub.set_text(&make_sub(wc::BINARY, self.label.borrow().as_deref()));
                self.search.replace(search.clone());
            }
        }
        if let Some(text) = &item.text {
            self.full_text.replace(Some(text.clone()));
            self.utf8.replace(item.utf8.clone());
            self.search.replace(search);
            if self.preview.contains('\0') {
                self.title.set_text(&make_title(&self.kind.borrow(), &title_source(&self.preview, Some(text))));
            }
        }
    }

    fn row_key(&self) -> RowKey {
        match (&self.pin, &self.entry) {
            (Some(p), _) => RowKey::Pin(p.sha.clone()),
            (None, Some(e)) => RowKey::Entry(key(e)),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum RowKey {
    Pin(String),
    Entry(Key),
}

/// Os bytes e o tipo da cópia. Um texto que a outra linha ainda não leu decide agora se é UTF-16 certo.
/// Um texto com bytes zero copiado como está vai como text/plain (o wl-copy deduziria octet-stream).
fn payload(row: &ClipRow, data: Vec<u8>, mime: Option<String>) -> (Vec<u8>, Option<String>) {
    if *row.kind.borrow() != wc::TEXT {
        return (data, mime);
    }
    if !row.content_read.get() {
        let content = read_content(&data);
        if let Some(utf8) = content.utf8.filter(|t| !t.is_empty()) {
            return (utf8.into_bytes(), Some(UTF8_TEXT.into()));
        }
        if content.kind != wc::TEXT {
            return (data, mime);
        }
    }
    if mime.is_none() && data.contains(&0) {
        return (data, Some("text/plain".into()));
    }
    (data, mime)
}

/// Os resultados da outra linha, entregues à janela pelo temporizador.
type Results = Arc<Mutex<Vec<(Task, Option<Indexed>)>>>;

struct Popup {
    popup: Rc<PopupWindow>,
    tools: Tools,
    pin_root: PathBuf,
    thumb_root: PathBuf,
    store: RefCell<PinStore>,
    entry: gtk4::SearchEntry,
    scroll: gtk4::ScrolledWindow,
    pinned_section: gtk4::Box,
    pinned_list: gtk4::ListBox,
    recent_section: gtk4::Box,
    recent_list: gtk4::ListBox,
    empty: gtk4::Label,
    status: gtk4::Label,
    count: gtk4::Label,
    clear_button: gtk4::Button,
    entries: RefCell<Vec<Entry>>,
    pins: RefCell<Vec<Pin>>,
    pinned_rows: RefCell<Vec<Rc<ClipRow>>>,
    recent_rows: RefCell<Vec<Rc<ClipRow>>>,
    visible_rows: RefCell<Vec<Rc<ClipRow>>>,
    row_by_key: RefCell<HashMap<RowKey, Rc<ClipRow>>>,
    /// (id, prévia) → o que a outra linha leu; sobrevive a fixar e apagar
    indexed: RefCell<HashMap<Key, Indexed>>,
    /// sha → o conteúdo de um fixado de texto
    pin_content: RefCell<HashMap<String, Indexed>>,
    /// Os que ainda não têm linha (veja FIRST_ROWS)
    pending: RefCell<Vec<Entry>>,
    rows_generation: Cell<u64>,
    generation: Arc<AtomicU64>,
    results: Results,
    cursor: RefCell<Option<Rc<ClipRow>>>,
    clear_confirm: RefCell<Confirm>,
    clear_timer: RefCell<Option<glib::SourceId>>,
    body: gtk4::Box,
    /// A restauração da rolagem em espera (veja restore_scroll)
    scroll_restore: RefCell<Option<ScrollRestore>>,
    /// (linha, topo) que o scroll_to espera a lista ter tamanho para mostrar
    scroll_target: RefCell<Option<(Rc<ClipRow>, bool)>>,
}

struct ScrollRestore {
    value: f64,
    /// A altura em que estava, na parte visível, a linha em que o usuário mexeu
    offset: Option<f64>,
    deadline: glib::SourceId,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, PROGRAM, &format!("{CSS_DEV}{CSS_LIST}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(CARD_WIDTH, -1);
        popup.window.set_child(Some(&card));
        let entry = gtk4::SearchEntry::new();
        entry.set_placeholder_text(Some(&tr("Search the history…")));
        card.append(&entry);
        let body = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let scroll = scroller(&body, SCROLL_HEIGHT);
        card.append(&scroll);
        let make_section = |title: &str| {
            let section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
            section.append(&label(title, &["section"], 0.0));
            let boxed = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            boxed.add_css_class("box");
            let list = gtk4::ListBox::new();
            list.set_selection_mode(gtk4::SelectionMode::Single);
            list.set_activate_on_single_click(true);
            list.set_focusable(false);
            list.add_css_class("clip-list");
            boxed.append(&list);
            section.append(&boxed);
            (section, list)
        };
        let (pinned_section, pinned_list) = make_section(&tr("Pinned"));
        let (recent_section, recent_list) = make_section(&tr("Recent"));
        let empty = empty_label("");
        empty.set_ellipsize(pango::EllipsizeMode::End); // a busca não tem limite de tamanho
        body.append(&pinned_section);
        body.append(&recent_section);
        body.append(&empty);
        let status = label(" ", &["status"], 0.0);
        status.set_ellipsize(pango::EllipsizeMode::End);
        card.append(&status);
        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let count = label("", &["key"], 0.0);
        foot.append(&count);
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);

        let cache_root = xdg("XDG_CACHE_HOME", ".cache").join("waybar-clipboard");
        let pin_root = xdg("XDG_DATA_HOME", ".local/share").join("waybar-clipboard");
        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let m = me.clone();
            let clear_button = link_button(&tr("Clear history"), &tr("Deletes the history; pinned items stay"), move || {
                if let Some(p) = m.upgrade() {
                    p.on_clear();
                }
            });
            clear_button.set_focusable(false);
            Popup {
                popup: popup.clone(),
                tools: Tools::default(),
                store: RefCell::new(PinStore::new(&pin_root)),
                pin_root: pin_root.clone(),
                thumb_root: cache_root.join("thumbs-v2"),
                entry,
                scroll,
                pinned_section,
                pinned_list,
                recent_section,
                recent_list,
                empty,
                status,
                count,
                clear_button,
                entries: RefCell::default(),
                pins: RefCell::default(),
                pinned_rows: RefCell::default(),
                recent_rows: RefCell::default(),
                visible_rows: RefCell::default(),
                row_by_key: RefCell::default(),
                indexed: RefCell::default(),
                pin_content: RefCell::default(),
                pending: RefCell::default(),
                rows_generation: Cell::new(0),
                generation: Arc::new(AtomicU64::new(0)),
                results: Results::default(),
                cursor: RefCell::default(),
                clear_confirm: RefCell::new(Confirm::new(Duration::from_secs(3))),
                clear_timer: RefCell::default(),
                body: body.clone(),
                scroll_restore: RefCell::default(),
                scroll_target: RefCell::default(),
            }
        });
        foot.append(&this.clear_button);
        card.append(&foot);
        // A pasta antiga tem miniaturas distorcidas: sai ao abrir
        let _ = std::fs::remove_dir_all(cache_root.join("thumbs"));

        for list in [&this.pinned_list, &this.recent_list] {
            let me = Rc::downgrade(&this);
            list.connect_row_activated(move |_, row| {
                if let Some(p) = me.upgrade() {
                    if let Some(r) = p.find_row(row) {
                        p.copy_row(&r);
                    }
                }
            });
        }
        let me = Rc::downgrade(&this);
        this.entry.connect_search_changed(move |_| {
            if let Some(p) = me.upgrade() {
                p.on_search_changed();
            }
        });
        // Quem rola a lista manda mais que uma restauração atrasada. Só observa, sem parar a rolagem
        let scrolled = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        scrolled.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = Rc::downgrade(&this);
        scrolled.connect_scroll(move |_, _, _| {
            if let Some(p) = me.upgrade() {
                p.cancel_scroll_restore();
            }
            glib::Propagation::Proceed
        });
        this.scroll.add_controller(scrolled);
        // As teclas na busca, antes do SearchEntry: setas e Tab andam, Enter copia, Esc sempre fecha
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(p) = me.upgrade() else { return glib::Propagation::Proceed };
            use gdk::Key;
            match key {
                Key::Down | Key::Tab => p.move_cursor(1),
                Key::Up | Key::ISO_Left_Tab => p.move_cursor(-1),
                Key::Return | Key::KP_Enter => {
                    let cursor = p.cursor.borrow().clone();
                    if let Some(row) = cursor {
                        p.copy_row(&row);
                    }
                }
                Key::Escape => p.popup.close("Esc"),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        this.entry.add_controller(keys);

        this.reload(true, None);
        this.popup.show();
        // O fim de cada layout do frame clock faz o papel do size-allocate do Python: as linhas novas já
        // têm posição e tamanho
        if let Some(clock) = this.popup.window.frame_clock() {
            let me = Rc::downgrade(&this);
            clock.connect_layout(move |_| {
                if let Some(p) = me.upgrade() {
                    p.on_layout();
                }
            });
        }
        this.apply_filter();
        this.entry.grab_focus();
        this.popup.rendered_at.set(Instant::now());
        this
    }

    fn set_status(&self, text: &str) {
        self.popup.rendered_at.set(Instant::now());
        set_status(&self.status, text);
    }

    fn find_row(&self, row: &gtk4::ListBoxRow) -> Option<Rc<ClipRow>> {
        self.pinned_rows.borrow().iter().chain(self.recent_rows.borrow().iter()).find(|r| &r.row == row).cloned()
    }

    fn sha_of(&self, entry: &Entry) -> Option<String> {
        self.indexed.borrow().get(&key(entry)).and_then(|i| i.sha.clone())
    }

    // --- dados ---
    /// `progressive`: só na abertura, a janela aparece antes de todas as linhas. `near`: a linha em que o
    /// usuário mexeu; o cursor vai para ela ou, se ela sumiu, para a seguinte.
    fn reload(self: &Rc<Self>, progressive: bool, near: Option<Rc<ClipRow>>) {
        let scroll = self.scroll.vadjustment().value();
        let anchor = near.or_else(|| self.cursor.borrow().clone());
        let keep = self.cursor_candidates(anchor.as_ref());
        let offset = self.screen_offset(anchor.as_ref());
        self.stop_worker();
        let (entries, mut error) = self.tools.list_entries();
        let current: HashSet<Key> = entries.iter().map(key).collect();
        self.indexed.borrow_mut().retain(|k, _| current.contains(k));
        let pins = self.store.borrow_mut().load();
        if !self.store.borrow().last_read_ok && self.store.borrow().meta_path.exists() {
            error = error.or(Some(tr("The pinned items file is unreadable; it will not be overwritten.")));
        }
        let shas: HashSet<String> = pins.iter().map(|p| p.sha.clone()).collect();
        self.pin_content.borrow_mut().retain(|sha, _| shas.contains(sha));
        self.entries.replace(entries);
        self.pins.replace(pins);
        self.build_rows(if progressive { Some(FIRST_ROWS) } else { None });
        if !progressive {
            self.apply_filter();
            self.restore_cursor(&keep);
            self.restore_scroll(scroll, offset);
        }
        if let Some(error) = error {
            self.set_status(&error);
        }
        self.prepare_thumbs();
        self.start_worker();
    }

    /// A linha e as seguintes (depois as anteriores, da mais próxima): a primeira que ainda existir
    /// depois da recarga recebe o cursor.
    fn cursor_candidates(&self, row: Option<&Rc<ClipRow>>) -> Vec<RowKey> {
        let visible = self.visible_rows.borrow();
        let Some(index) = row.and_then(|r| visible.iter().position(|v| Rc::ptr_eq(v, r))) else { return Vec::new() };
        let pins = self.pins.borrow();
        let mut keys: Vec<RowKey> = visible[index..].iter().map(|r| r.row_key()).collect();
        keys.extend(self.pending.borrow().iter().filter(|e| wc::recent_visible(e, &pins, self.sha_of(e).as_deref())).map(|e| RowKey::Entry(key(e))));
        keys.extend(visible[..index].iter().rev().map(|r| r.row_key()));
        keys
    }

    fn restore_cursor(&self, keys: &[RowKey]) {
        let visible = self.visible_rows.borrow().clone();
        let by_key = self.row_by_key.borrow();
        for k in keys {
            if let Some(row) = by_key.get(k).filter(|r| visible.iter().any(|v| Rc::ptr_eq(v, r))) {
                self.set_cursor(Some(row.clone()));
                return;
            }
        }
    }

    /// A altura da linha na parte visível da lista, ou None se ela não estiver na tela.
    fn screen_offset(&self, row: Option<&Rc<ClipRow>>) -> Option<f64> {
        let row = row?;
        if !self.visible_rows.borrow().iter().any(|v| Rc::ptr_eq(v, row)) {
            return None;
        }
        let (y, height) = self.position_in_body(row)?;
        let adjustment = self.scroll.vadjustment();
        let offset = y - adjustment.value();
        (offset >= 0.0 && offset + height <= adjustment.page_size()).then_some(offset)
    }

    /// O topo e a altura da linha no conteúdo da lista (não na parte visível), se ela já tiver tamanho.
    fn position_in_body(&self, row: &Rc<ClipRow>) -> Option<(f64, f64)> {
        let height = row.row.height();
        if height <= 1 || row.row.parent().is_none() {
            return None;
        }
        let point = row.row.compute_point(&self.body, &gtk4::graphene::Point::new(0.0, 0.0))?;
        Some((f64::from(point.y()), f64::from(height)))
    }

    /// Só depois de as linhas novas terem tamanho (o próximo layout): antes, o GTK cortaria o valor pela
    /// lista antiga. `offset`: a linha do cursor volta para essa altura, mesmo que a seção Fixados tenha
    /// surgido acima e empurrado a lista. A busca, as setas e a rolagem do usuário cancelam.
    fn restore_scroll(self: &Rc<Self>, value: f64, offset: Option<f64>) {
        self.cancel_scroll_restore();
        let me = Rc::downgrade(self);
        let deadline = glib::timeout_add_local_once(SCROLL_RESTORE, move || {
            if let Some(p) = me.upgrade() {
                p.scroll_restore.replace(None); // o prazo já terminou: nada a remover
            }
        });
        self.scroll_restore.replace(Some(ScrollRestore { value, offset, deadline }));
    }

    fn cancel_scroll_restore(&self) {
        self.scroll_target.replace(None); // uma rolagem adiada também perde a vez (busca, setas, mouse)
        if let Some(restore) = self.scroll_restore.take() {
            restore.deadline.remove();
        }
    }

    fn on_layout(&self) {
        let pending = self.scroll_restore.borrow().as_ref().map(|r| (r.value, r.offset));
        if let Some((value, offset)) = pending {
            let adjustment = self.scroll.vadjustment();
            let cursor = self.cursor.borrow().clone();
            let position = cursor.as_ref().and_then(|c| self.position_in_body(c));
            if let (Some(offset), Some((y, _))) = (offset, position) {
                // Segue até o prazo: a lista ainda pode mudar de altura no layout seguinte
                adjustment.set_value(y - offset);
                return;
            }
            self.cancel_scroll_restore();
            adjustment.set_value(value);
            if let Some(row) = cursor {
                self.scroll_to(&row, false);
            }
            return;
        }
        let target = self.scroll_target.borrow().clone();
        if let Some((row, top)) = target {
            if row.row.parent().is_some() {
                self.scroll_to(&row, top);
            } else {
                self.scroll_target.replace(None);
            }
        }
    }

    fn build_rows(self: &Rc<Self>, first: Option<usize>) {
        self.cursor.replace(None);
        for list in [&self.pinned_list, &self.recent_list] {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
        }
        self.pinned_rows.borrow_mut().clear();
        self.recent_rows.borrow_mut().clear();
        self.row_by_key.borrow_mut().clear();
        self.rows_generation.set(self.rows_generation.get() + 1);
        let pins = self.pins.borrow().clone();
        for pin in pins {
            let item = self.pin_content.borrow().get(&pin.sha).cloned();
            let row = ClipRow::new(None, Some(pin), item.as_ref());
            self.add_row(row, true);
        }
        let entries = self.entries.borrow().clone();
        let cut = first.unwrap_or(entries.len()).min(entries.len());
        self.add_recent_rows(&entries[..cut]);
        self.pending.replace(entries[cut..].to_vec());
        self.popup.rendered_at.set(Instant::now());
        if !self.pending.borrow().is_empty() {
            // Depois do primeiro quadro: um bloco antes dele atrasaria a janela
            let (me, generation) = (Rc::downgrade(self), self.rows_generation.get());
            glib::timeout_add_local_once(Duration::from_millis(50), move || {
                if let Some(p) = me.upgrade() {
                    p.add_pending_rows(generation);
                }
            });
        }
    }

    fn add_recent_rows(self: &Rc<Self>, entries: &[Entry]) {
        for entry in entries {
            let item = self.indexed.borrow_mut().get_mut(&key(entry)).map(|i| {
                let copy = i.clone();
                i.thumb = None; // a miniatura vai para a linha; o cache fica em disco
                copy
            });
            let row = ClipRow::new(Some(entry.clone()), None, item.as_ref());
            if let Some(png) = item.as_ref().and_then(|i| i.thumb.as_ref()) {
                row.set_thumb(png);
            }
            self.add_row(row, false);
        }
    }

    fn add_row(self: &Rc<Self>, row: Rc<ClipRow>, pinned: bool) {
        let me = Rc::downgrade(self);
        let r = Rc::downgrade(&row);
        row.pin_button.connect_clicked(move |_| {
            if let (Some(p), Some(r)) = (me.upgrade(), r.upgrade()) {
                p.on_pin(&r);
            }
        });
        let me = Rc::downgrade(self);
        let r = Rc::downgrade(&row);
        row.trash_button.connect_clicked(move |_| {
            if let (Some(p), Some(r)) = (me.upgrade(), r.upgrade()) {
                p.on_remove(&r);
            }
        });
        let (list, rows) = if pinned { (&self.pinned_list, &self.pinned_rows) } else { (&self.recent_list, &self.recent_rows) };
        list.append(&row.row);
        self.row_by_key.borrow_mut().insert(row.row_key(), row.clone());
        rows.borrow_mut().push(row);
    }

    /// As linhas que faltam, um bloco por vez, com a janela já na tela.
    fn add_pending_rows(self: &Rc<Self>, generation: u64) {
        if generation != self.rows_generation.get() || self.pending.borrow().is_empty() {
            return;
        }
        if !wc::terms(&self.entry.text()).is_empty() {
            self.flush_pending(); // digitou, e o search-changed ainda não chegou
            self.apply_filter();
            return;
        }
        let chunk: Vec<Entry> = {
            let mut pending = self.pending.borrow_mut();
            let n = pending.len().min(MORE_ROWS);
            pending.drain(..n).collect()
        };
        let start = self.recent_rows.borrow().len();
        self.add_recent_rows(&chunk);
        let pins = self.pins.borrow().clone();
        let new_rows: Vec<Rc<ClipRow>> = self.recent_rows.borrow()[start..].to_vec();
        for row in &new_rows {
            let visible = row.entry.as_ref().is_some_and(|e| wc::recent_visible(e, &pins, self.sha_of(e).as_deref()));
            row.row.set_visible(visible);
            if visible {
                self.visible_rows.borrow_mut().push(row.clone());
            }
        }
        if new_rows.iter().any(|r| r.row.is_visible()) {
            self.recent_section.set_visible(true);
        }
        let first = self.visible_rows.borrow().first().cloned();
        let cursor_shown = self.cursor.borrow().as_ref().is_some_and(|c| self.visible_rows.borrow().iter().any(|v| Rc::ptr_eq(v, c)));
        if !cursor_shown && first.is_some() {
            self.set_cursor(first);
        }
        if self.pending.borrow().is_empty() {
            self.apply_filter(); // o último bloco: a lista inteira, com o aviso de vazia se for o caso
            return;
        }
        self.update_count(self.visible_rows.borrow().len(), "");
        let me = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(p) = me.upgrade() {
                p.add_pending_rows(generation);
            }
        });
    }

    fn flush_pending(self: &Rc<Self>) {
        let chunk: Vec<Entry> = self.pending.borrow_mut().drain(..).collect();
        if !chunk.is_empty() {
            self.add_recent_rows(&chunk);
        }
    }

    fn pending_count(&self) -> usize {
        let pins = self.pins.borrow();
        self.pending.borrow().iter().filter(|e| wc::recent_visible(e, &pins, self.sha_of(e).as_deref())).count()
    }

    fn apply_filter(&self) {
        let query = self.entry.text().to_string();
        let wanted = wc::terms(&query);
        let pins = self.pins.borrow().clone();
        let mut pinned = Vec::new();
        let mut recent = Vec::new();
        for row in self.pinned_rows.borrow().iter() {
            let visible = wanted.is_empty() || wc::matches_terms(&wanted, &row.search_text());
            row.row.set_visible(visible);
            if visible {
                pinned.push(row.clone());
            }
        }
        for row in self.recent_rows.borrow().iter() {
            let mut visible = wanted.is_empty() || wc::matches_terms(&wanted, &row.search_text());
            if visible && row.entry.as_ref().is_some_and(|e| !wc::recent_visible(e, &pins, self.sha_of(e).as_deref())) {
                visible = false;
            }
            row.row.set_visible(visible);
            if visible {
                recent.push(row.clone());
            }
        }
        self.pinned_section.set_visible(!pinned.is_empty());
        self.recent_section.set_visible(!recent.is_empty());
        let total = pinned.len() + recent.len();
        self.empty.set_visible(total == 0 && self.pending.borrow().is_empty());
        let rows: Vec<Rc<ClipRow>> = pinned.into_iter().chain(recent).collect();
        self.visible_rows.replace(rows.clone());
        self.update_count(total, &query);
        let cursor_shown = self.cursor.borrow().as_ref().is_some_and(|c| rows.iter().any(|v| Rc::ptr_eq(v, c)));
        if !cursor_shown {
            self.set_cursor(rows.first().cloned());
        }
    }

    fn update_count(&self, total: usize, query: &str) {
        let searching = !wc::terms(query).is_empty();
        if total == 0 {
            self.empty.set_text(&if searching { tr("Nothing found for “%s”").replace("%s", query) } else { tr("History is empty") });
        }
        let total = if searching { total } else { total + self.pending_count() };
        self.count.set_text(&ntr("%d item", "%d items", total as u64));
    }

    fn set_cursor(&self, row: Option<Rc<ClipRow>>) {
        for list in [&self.pinned_list, &self.recent_list] {
            match &row {
                Some(r) if r.row.parent().as_ref() == Some(list.upcast_ref()) => list.select_row(Some(&r.row)),
                _ => list.unselect_all(),
            }
        }
        self.cursor.replace(row);
    }

    // --- índice e miniaturas ---
    fn pinned_images(&self) -> HashSet<String> {
        self.pins.borrow().iter().filter(|p| p.kind == wc::IMAGE).map(|p| p.sha.clone()).collect()
    }

    fn prepare_thumbs(&self) {
        use std::os::unix::fs::PermissionsExt;
        for dir in [self.thumb_root.parent().unwrap_or(&self.thumb_root).to_path_buf(), self.thumb_root.clone()] {
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        wc::prune_thumbs(&self.thumb_root, wc::THUMB_KEEP, &self.pinned_images());
    }

    fn start_worker(self: &Rc<Self>) {
        // As imagens sempre entram: a miniatura não fica guardada na memória entre as recargas
        let mut tasks: Vec<Task> = self
            .pinned_rows
            .borrow()
            .iter()
            .filter_map(|r| r.pin.clone())
            .filter(|p| p.kind == wc::IMAGE || (p.kind == wc::TEXT && !self.pin_content.borrow().contains_key(&p.sha)))
            .map(Task::Pin)
            .collect();
        for entry in self.entries.borrow().iter() {
            if entry.kind == wc::IMAGE || !self.indexed.borrow().contains_key(&key(entry)) {
                tasks.push(Task::Entry(entry.clone(), self.sha_of(entry)));
            }
        }
        if tasks.is_empty() {
            return;
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let (current, results, tools, pin_root, thumbs) = (self.generation.clone(), self.results.clone(), self.tools.clone(), self.pin_root.clone(), self.thumb_root.clone());
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = done.clone();
        std::thread::spawn(move || {
            let mut batch = Vec::new();
            let mut sent_at = Instant::now();
            for task in tasks {
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                let item = index_item(&tools, &pin_root, &thumbs, &task);
                batch.push((task, item));
                if batch.len() >= INDEX_BATCH || sent_at.elapsed() >= INDEX_BATCH_TIME {
                    results.lock().map(|mut r| r.append(&mut batch)).ok();
                    sent_at = Instant::now();
                }
            }
            results.lock().map(|mut r| r.append(&mut batch)).ok();
            finished.store(true, Ordering::SeqCst);
        });
        let me = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(60), move || {
            let Some(p) = me.upgrade() else { return glib::ControlFlow::Break };
            if p.generation.load(Ordering::SeqCst) != generation {
                return glib::ControlFlow::Break;
            }
            // Lido antes de drenar: o que chegou até o fim já está na lista
            let last = done.load(Ordering::SeqCst);
            let batch: Vec<(Task, Option<Indexed>)> = p.results.lock().map(|mut r| r.drain(..).collect()).unwrap_or_default();
            if !batch.is_empty() {
                p.apply_batch(batch);
            }
            if last { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
        });
    }

    /// Depois disto a outra linha não entrega mais resultados; no máximo uma miniatura em andamento termina
    /// de ser gravada (do conteúdo certo, e o prune a limpa depois).
    fn stop_worker(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut r) = self.results.lock() {
            r.clear();
        }
    }

    fn apply_batch(&self, batch: Vec<(Task, Option<Indexed>)>) {
        for (task, item) in batch {
            let Some(item) = item else { continue };
            let row = match &task {
                Task::Pin(pin) => {
                    if pin.kind == wc::TEXT {
                        self.pin_content.borrow_mut().insert(pin.sha.clone(), Indexed { thumb: None, ..item.clone() });
                    }
                    self.row_by_key.borrow().get(&RowKey::Pin(pin.sha.clone())).cloned()
                }
                Task::Entry(entry, _) => {
                    self.indexed.borrow_mut().insert(key(entry), item.clone());
                    self.row_by_key.borrow().get(&RowKey::Entry(key(entry))).cloned()
                }
            };
            let Some(row) = row else { continue };
            row.set_indexed(&item);
            if let Some(png) = &item.thumb {
                row.set_thumb(png);
                if let Task::Entry(entry, _) = &task {
                    if let Some(i) = self.indexed.borrow_mut().get_mut(&key(entry)) {
                        i.thumb = None;
                    }
                }
            }
        }
        self.apply_filter();
    }

    // --- ações ---
    fn copy_row(&self, row: &Rc<ClipRow>) {
        let result = (|| -> Result<(), ClipboardError> {
            // Vazio conta como "sem conversão", como o `if row.utf8:` do Python (um item só com o BOM)
            let utf8 = row.utf8.borrow().clone().filter(|t| !t.is_empty());
            if let Some(text) = utf8 {
                // Texto em UTF-16: um app em UTF-8 pararia na primeira letra
                return self.tools.copy(text.as_bytes(), Some(UTF8_TEXT));
            }
            let (data, mime) = match (&row.pin, &row.entry) {
                (Some(pin), _) => {
                    let path = self.store.borrow().blob_path(&pin.sha)?;
                    (std::fs::read(path).map_err(|e| ClipboardError(e.to_string()))?, pin.mime.clone())
                }
                (None, Some(entry)) => {
                    let data = self.tools.decode(&entry.id)?;
                    if data.is_empty() {
                        return Err(ClipboardError(tr("This item is no longer in the history.")));
                    }
                    (data, entry.mime.clone())
                }
                _ => return Ok(()),
            };
            let (data, mime) = payload(row, data, mime);
            self.tools.copy(&data, mime.as_deref())
        })();
        match result {
            Ok(()) => self.popup.close("copiou"),
            Err(e) => self.set_status(&tr("Could not copy: %s").replace("%s", &e.0)),
        }
    }

    fn on_pin(self: &Rc<Self>, row: &Rc<ClipRow>) {
        self.set_status("");
        if let Some(pin) = &row.pin {
            self.unpin(pin, row);
            return;
        }
        let Some(entry) = row.entry.clone() else { return };
        let result = (|| -> Result<(), ClipboardError> {
            let data = self.tools.decode(&entry.id)?;
            if data.is_empty() {
                return Err(ClipboardError(tr("This item is no longer in the history.")));
            }
            let mut entry = entry.clone();
            if entry.kind == wc::TEXT {
                // A prévia do cliphist 0.5 não marca binário: o conteúdo diz
                entry.kind = read_content(&data).kind;
            }
            self.store.borrow_mut().pin(&entry, &data, None)?;
            Ok(())
        })();
        match result {
            Ok(()) => self.set_status(&ptr("clipboard status", "Pinned")),
            Err(e) => {
                self.set_status(&tr("Could not pin: %s").replace("%s", &e.0));
                return;
            }
        }
        self.reload(false, Some(row.clone()));
    }

    fn unpin(self: &Rc<Self>, pin: &Pin, row: &Rc<ClipRow>) {
        self.stop_worker();
        match self.store.borrow_mut().unpin(&pin.sha) {
            Ok(_) => {
                if pin.kind == wc::IMAGE {
                    wc::remove_thumb(&self.thumb_root, &pin.sha); // se ainda estiver no histórico, é refeita
                }
                self.set_status(&tr("Removed from pinned"));
            }
            Err(e) => self.set_status(&tr("Could not unpin: %s").replace("%s", &e.0)),
        }
        self.reload(false, Some(row.clone()));
    }

    fn on_remove(self: &Rc<Self>, row: &Rc<ClipRow>) {
        self.set_status("");
        if let Some(pin) = row.pin.clone() {
            self.unpin(&pin, row);
            return;
        }
        let Some(entry) = row.entry.clone() else { return };
        self.stop_worker();
        // O sha é o nome da miniatura: precisa ser lido antes de o item sumir do histórico
        let digest = (entry.kind == wc::IMAGE).then(|| self.sha_of(&entry).or_else(|| self.tools.decode(&entry.id).ok().filter(|d| !d.is_empty()).map(|d| wc::sha256(&d)))).flatten();
        match self.tools.delete(&entry.id) {
            Ok(()) => {
                if let Some(sha) = digest.filter(|s| !self.pinned_images().contains(s)) {
                    wc::remove_thumb(&self.thumb_root, &sha);
                }
                self.set_status(&tr("Deleted from history"));
            }
            Err(e) => self.set_status(&tr("Could not delete: %s").replace("%s", &e.0)),
        }
        self.reload(false, Some(row.clone()));
    }

    fn on_clear(self: &Rc<Self>) {
        if !self.clear_confirm.borrow_mut().click(Instant::now()) {
            self.set_clear_armed(true);
            if let Some(t) = self.clear_timer.take() {
                t.remove();
            }
            let me = Rc::downgrade(self);
            let timer = glib::timeout_add_local_once(Duration::from_secs(3), move || {
                if let Some(p) = me.upgrade() {
                    p.clear_timer.take();
                    p.reset_clear();
                }
            });
            self.clear_timer.replace(Some(timer));
            return;
        }
        self.reset_clear();
        self.stop_worker();
        match self.tools.wipe() {
            Ok(()) => {
                self.indexed.borrow_mut().clear(); // o cliphist pode reutilizar os IDs depois de um wipe
                wc::prune_thumbs(&self.thumb_root, 0, &self.pinned_images());
                self.set_status(&tr("History cleared"));
            }
            Err(e) => self.set_status(&tr("Could not clear: %s").replace("%s", &e.0)),
        }
        self.reload(false, None);
    }

    fn set_clear_armed(&self, armed: bool) {
        self.clear_button.set_label(&if armed { tr("Confirm: clear history") } else { tr("Clear history") });
        if armed {
            self.clear_button.add_css_class("danger");
        } else {
            self.clear_button.remove_css_class("danger");
        }
    }

    fn reset_clear(&self) {
        if let Some(t) = self.clear_timer.take() {
            t.remove();
        }
        self.clear_confirm.borrow_mut().reset();
        self.set_clear_armed(false);
    }

    // --- teclado ---
    fn on_search_changed(self: &Rc<Self>) {
        self.cancel_scroll_restore(); // a busca volta ao topo; uma restauração atrasada desfaria isso
        self.flush_pending();
        self.cursor.replace(None); // a seleção volta ao primeiro resultado
        self.apply_filter();
        self.scroll.vadjustment().set_value(0.0);
    }

    fn move_cursor(&self, delta: i32) {
        self.cancel_scroll_restore(); // o cursor andou: a restauração prenderia a linha antiga
        let rows = self.visible_rows.borrow().clone();
        if rows.is_empty() {
            return;
        }
        let current = self.cursor.borrow().as_ref().and_then(|c| rows.iter().position(|r| Rc::ptr_eq(r, c))).unwrap_or(0) as i32;
        let index = (current + delta).clamp(0, rows.len() as i32 - 1) as usize;
        self.set_cursor(Some(rows[index].clone()));
        self.scroll_to(&rows[index], index == 0);
    }

    /// A lista não rola sozinha: o foco nunca sai da busca. Uma linha recém-criada (logo depois de fixar
    /// ou apagar) ainda não tem posição: a rolagem espera o próximo layout.
    fn scroll_to(&self, row: &Rc<ClipRow>, top: bool) {
        self.scroll_target.replace(None);
        let adjustment = self.scroll.vadjustment();
        if top {
            adjustment.set_value(0.0); // no primeiro, o título da seção também aparece
            return;
        }
        let Some((y, height)) = self.position_in_body(row) else {
            self.scroll_target.replace(Some((row.clone(), top)));
            return;
        };
        let bottom = y + height;
        if y < adjustment.value() {
            adjustment.set_value(y);
        } else if bottom > adjustment.value() + adjustment.page_size() {
            adjustment.set_value(bottom - adjustment.page_size());
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-clipboard-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), || {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_subtitles() {
        assert_eq!(clean_preview("  a\tb\n c\u{1c}d "), "a b c d");
        assert_eq!(clean_preview(""), "(vazio)");
        assert_eq!(make_sub(wc::IMAGE, Some("image/png")), "PNG");
        let utf16: String = "https://x".chars().flat_map(|c| [c, '\0']).collect();
        assert_eq!(title_source(&utf16, Some("https://x — Título")), "https://x — Título");
        assert_eq!(title_source("normal", Some("outro")), "normal");
    }
}
