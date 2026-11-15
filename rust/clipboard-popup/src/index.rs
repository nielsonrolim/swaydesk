//! O que roda fora da linha principal: ler cada item (um processo do cliphist por item), decidir o tipo
//! pelo conteúdo e fazer as miniaturas. O porte do `index_item`, `load_thumb` e `render_thumb` do
//! `waybar-clipboard-popup`. Nada de GTK aqui além do GdkPixbuf, que é seguro em outra linha.
use std::path::{Path, PathBuf};

use gtk4::gdk_pixbuf::{self, prelude::*, Pixbuf};
use swaydesk_clipboard::{self as wc, read_content, sha256, Content, Entry, Pin, PinStore, Tools};

pub const THUMB_SIZE: i32 = 80;

/// O que a outra linha descobriu sobre um item.
#[derive(Clone, Default)]
pub struct Indexed {
    pub sha: Option<String>,
    pub text: Option<String>,
    pub search: Option<String>,
    /// A miniatura em PNG: o Pixbuf não passa de uma linha para outra, os bytes sim
    pub thumb: Option<Vec<u8>>,
    /// O conteúdo é binário: o rótulo dele ("PDF"), ou "" sem rótulo
    pub binary: Option<String>,
    /// O texto a copiar no lugar dos bytes (UTF-16 certo; veja Content)
    pub utf8: Option<String>,
}

/// Uma tarefa: um item do histórico (com o sha, se já se sabe) ou um fixado.
#[derive(Clone)]
pub enum Task {
    Entry(Entry, Option<String>),
    Pin(Pin),
}

fn indexed_content(sha: String, content: Content, search: Option<String>) -> Indexed {
    if content.kind == wc::BINARY {
        return Indexed { sha: Some(sha), search, binary: Some(content.label.unwrap_or_default()), ..Default::default() };
    }
    Indexed { sha: Some(sha), text: content.text, search, utf8: content.utf8, ..Default::default() }
}

/// Miniatura que cabe em THUMB_SIZE, com a proporção da imagem e sem ampliar.
pub fn render_thumb(data: &[u8]) -> Option<Pixbuf> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.connect_size_prepared(|loader, w, h| {
        let (w, h) = wc::scale_box(w, h, THUMB_SIZE);
        loader.set_size(w, h);
    });
    let written = loader.write(data);
    let closed = loader.close();
    written.and(closed).ok()?;
    loader.pixbuf()
}

/// O PNG da miniatura: do cache ou gerado de novo; um arquivo do cache ilegível é substituído.
pub fn load_thumb(root: &Path, sha: &str, read: impl FnOnce() -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let path = wc::thumb_path(root, sha).ok()?;
    if Pixbuf::from_file(&path).is_ok() {
        if let Ok(png) = std::fs::read(&path) {
            return Some(png);
        }
    }
    wc::remove_thumb(root, sha);
    let pixbuf = render_thumb(&read()?)?;
    let png = pixbuf.save_to_bufferv("png", &[]).ok()?;
    let _ = swaydesk_common::text::atomic_write(&path, &png, 0o600);
    Some(png)
}

/// Lê um item; None se ele não puder ser lido.
pub fn index_item(tools: &Tools, store_root: &Path, thumbs: &Path, task: &Task) -> Option<Indexed> {
    match task {
        Task::Pin(pin) => {
            let store = PinStore::new(store_root);
            let blob = store.blob_path(&pin.sha).ok()?;
            let read = || std::fs::read(&blob).ok();
            if pin.kind == wc::TEXT {
                return Some(indexed_content(pin.sha.clone(), read_content(&read()?), None));
            }
            Some(Indexed { sha: Some(pin.sha.clone()), thumb: load_thumb(thumbs, &pin.sha, read), ..Default::default() })
        }
        Task::Entry(entry, known) => {
            if let (true, Some(sha)) = (entry.kind == wc::IMAGE, known) {
                return Some(Indexed { sha: Some(sha.clone()), thumb: load_thumb(thumbs, sha, || tools.decode(&entry.id).ok()), ..Default::default() });
            }
            let data = tools.decode(&entry.id).ok().filter(|d| !d.is_empty())?;
            let digest = sha256(&data);
            if entry.kind == wc::IMAGE {
                return Some(Indexed { thumb: load_thumb(thumbs, &digest, || Some(data)), sha: Some(digest), ..Default::default() });
            }
            if entry.kind != wc::TEXT {
                return Some(Indexed { sha: Some(digest), ..Default::default() });
            }
            let content = read_content(&data);
            let item = Entry { kind: content.kind.clone(), mime: content.label.clone(), ..entry.clone() };
            let search = wc::haystack(&wc::search_fields(&item, content.text.as_deref()));
            Some(indexed_content(digest, content, Some(search)))
        }
    }
}

/// As pastas: `XDG_DATA_HOME`/`XDG_CACHE_HOME`, como o Python.
pub fn xdg(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| swaydesk_common::config::home().join(fallback))
}
