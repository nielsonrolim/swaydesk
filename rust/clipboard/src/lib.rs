//! A lógica do popup de clipboard, sem GTK: o porte de `home/.local/bin/waybar_clipboard.py` (cliphist,
//! busca, conteúdo, fixados, miniaturas e confirmação). A janela fica no swaydesk-clipboard-popup.
//!
//! Os arquivos são os mesmos do Python, para as duas versões conviverem: o `pins.json` e os blobs em
//! `~/.local/share/waybar-clipboard`, e as miniaturas em `~/.cache/waybar-clipboard/thumbs-v2`. Os tipos
//! gravados são os do Python ("texto", "imagem", "binário").
pub mod content;
pub mod pins;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use swaydesk_common::i18n::tr;

pub use content::{read_content, Content};
pub use pins::{Pin, PinStore};

pub const TEXT: &str = "texto";
pub const IMAGE: &str = "imagem";
pub const BINARY: &str = "binário";
pub const KINDS: [&str; 3] = [TEXT, IMAGE, BINARY];
const COPY_TIMEOUT: Duration = Duration::from_secs(5);
pub const THUMB_KEEP: usize = 500;

/// Falha ao falar com o cliphist, o wl-copy ou o armazenamento de fixados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(pub String);

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Uma linha do `cliphist list`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry {
    pub id: String,
    pub preview: String,
    pub kind: String,
    pub mime: Option<String>,
}

impl Entry {
    pub fn new(id: &str, preview: &str, kind: &str, mime: Option<&str>) -> Entry {
        Entry { id: id.into(), preview: preview.into(), kind: kind.into(), mime: mime.map(String::from) }
    }
}

pub fn sha256(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn is_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A prévia de imagem do cliphist 0.5: "[[ binary data 78 KiB png 300x200 ]]" → o formato.
fn binary_05_format(preview: &str) -> Option<&str> {
    let inner = preview.strip_prefix("[[ binary data ")?.strip_suffix(" ]]")?;
    let parts: Vec<&str> = inner.split(' ').collect();
    let [size, unit, format, dims] = parts.as_slice() else { return None };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (w, h) = dims.split_once('x')?;
    (digits(size) && matches!(*unit, "B" | "KiB" | "MiB") && !format.is_empty() && format.bytes().all(|b| b.is_ascii_lowercase()) && digits(w) && digits(h))
        .then_some(*format)
}

/// O tipo e o MIME a partir da prévia do cliphist.
pub fn classify(preview: &str) -> (&'static str, Option<String>) {
    if let Some(rest) = preview.strip_prefix("binary data ") {
        let mime = rest.trim();
        let mime = (!mime.is_empty()).then(|| mime.to_string());
        let kind = if mime.as_deref().is_some_and(|m| m.starts_with("image/")) { IMAGE } else { BINARY };
        return (kind, mime);
    }
    if let Some(format) = binary_05_format(preview) {
        return (IMAGE, Some(format!("image/{format}")));
    }
    if preview.starts_with("(conteúdo binário)") {
        return (BINARY, None);
    }
    (TEXT, None)
}

/// Uma linha `<id>\t<prévia>`; None se inválida.
pub fn parse_line(line: &str) -> Option<Entry> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (id, preview) = line.split_once('\t')?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (kind, mime) = classify(preview);
    Some(Entry { id: id.into(), preview: preview.into(), kind: kind.into(), mime })
}

/// A saída inteira do `cliphist list`, sem deixar bytes inválidos derrubarem a lista.
pub fn parse_list(data: &[u8]) -> Vec<Entry> {
    String::from_utf8_lossy(data).split('\n').filter_map(parse_line).collect()
}

/// Os programas: os nomes de sempre, ou outros nos testes.
#[derive(Clone, Debug)]
pub struct Tools {
    pub cliphist: PathBuf,
    pub wl_copy: PathBuf,
}

impl Default for Tools {
    fn default() -> Tools {
        Tools { cliphist: "cliphist".into(), wl_copy: "wl-copy".into() }
    }
}

fn could_not_run(program: &Path, err: &std::io::Error) -> ClipboardError {
    // O strerror do Python: a mensagem do sistema, sem o " (os error 2)" do Rust
    let text = err.to_string();
    let reason = match text.rfind(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    };
    ClipboardError(tr("Could not run %(program)s: %(error)s").replace("%(program)s", &program.to_string_lossy()).replace("%(error)s", &reason))
}

impl Tools {
    fn cliphist(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, ClipboardError> {
        let mut child = Command::new(&self.cliphist)
            .args(args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| could_not_run(&self.cliphist, &e))?;
        if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
            let _ = stdin.write_all(data);
        }
        let out = child.wait_with_output().map_err(|e| could_not_run(&self.cliphist, &e))?;
        if !out.status.success() {
            let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let fallback = || tr("%s failed").replace("%s", &format!("cliphist {}", args.join(" ")));
            return Err(ClipboardError(if message.is_empty() { fallback() } else { message }));
        }
        Ok(out.stdout)
    }

    /// As entradas e uma mensagem de erro opcional, sem falhar.
    pub fn list_entries(&self) -> (Vec<Entry>, Option<String>) {
        match self.cliphist(&["list"], None) {
            Ok(out) => (parse_list(&out), None),
            Err(e) => (Vec::new(), Some(e.0)),
        }
    }

    pub fn decode(&self, id: &str) -> Result<Vec<u8>, ClipboardError> {
        self.cliphist(&["decode"], Some(&id_input(id)))
    }

    pub fn delete(&self, id: &str) -> Result<(), ClipboardError> {
        self.cliphist(&["delete"], Some(&id_input(id))).map(|_| ())
    }

    pub fn wipe(&self) -> Result<(), ClipboardError> {
        self.cliphist(&["wipe"], None).map(|_| ())
    }

    /// Copia bytes para o wl-copy; vazio nunca chega ao clipboard. O wl-copy deixa um processo em
    /// segundo plano servindo o clipboard, que herda a saída: ela vai para um arquivo, e não para um pipe
    /// (esperar o fim dele travaria até a próxima cópia).
    pub fn copy(&self, data: &[u8], mime: Option<&str>) -> Result<(), ClipboardError> {
        if data.is_empty() {
            return Err(ClipboardError(tr("The item is empty.")));
        }
        // Na pasta do usuário (XDG_RUNTIME_DIR), e não no /tmp compartilhado
        let errors_path = swaydesk_common::resident::runtime_dir().join(format!(".swaydesk-wl-copy.{}.{:?}", std::process::id(), Instant::now()));
        let errors = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&errors_path).map_err(|e| could_not_run(&self.wl_copy, &e))?;
        let _ = std::fs::remove_file(&errors_path); // o arquivo some com o último descritor
        let mut command = Command::new(&self.wl_copy);
        if let Some(mime) = mime {
            command.args(["--type", mime]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(errors.try_clone().map_err(|e| could_not_run(&self.wl_copy, &e))?)
            .spawn()
            .map_err(|e| could_not_run(&self.wl_copy, &e))?;
        let mut stdin = child.stdin.take();
        let payload = data.to_vec();
        let writer = std::thread::spawn(move || {
            if let Some(stdin) = stdin.as_mut() {
                let _ = stdin.write_all(&payload);
            }
        });
        let deadline = Instant::now() + COPY_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ClipboardError(tr("%s did not answer").replace("%s", "wl-copy")));
                }
            }
        };
        let _ = writer.join();
        if !status.success() {
            let mut message = String::new();
            let mut file = errors;
            use std::io::Seek;
            let _ = file.seek(std::io::SeekFrom::Start(0));
            let _ = file.read_to_string(&mut message);
            let message = message.trim();
            return Err(ClipboardError(if message.is_empty() { tr("%s failed").replace("%s", "wl-copy") } else { message.to_string() }));
        }
        Ok(())
    }

    pub fn copy_entry(&self, entry: &Entry) -> Result<(), ClipboardError> {
        let data = self.decode(&entry.id)?;
        if data.is_empty() {
            return Err(ClipboardError(tr("This item is no longer in the history.")));
        }
        self.copy(&data, entry.mime.as_deref())
    }
}

/// O cliphist espera `<id>\t\n` na entrada de decode/delete.
pub fn id_input(id: &str) -> Vec<u8> {
    format!("{id}\t\n").into_bytes()
}

// --- busca ---

pub use swaydesk_common::text::normalize;

pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(normalize).filter(|t| !t.is_empty()).collect()
}

/// O texto normalizado de uma linha: calcular uma vez evita refazer tudo a cada filtro.
pub fn haystack<S: AsRef<str>>(fields: &[S]) -> String {
    let joined: Vec<&str> = fields.iter().map(AsRef::as_ref).filter(|f| !f.is_empty()).collect();
    normalize(&joined.join(" "))
}

pub fn matches_terms(wanted: &[String], text: &str) -> bool {
    wanted.iter().all(|t| text.contains(t.as_str()))
}

/// Todos os termos precisam aparecer; a busca vazia casa tudo.
pub fn matches<S: AsRef<str>>(query: &str, fields: &[S]) -> bool {
    let wanted = terms(query);
    wanted.is_empty() || matches_terms(&wanted, &haystack(fields))
}

/// A prévia para mostrar e buscar: o GTK corta o texto no primeiro byte zero, e um texto em UTF-16 tem um
/// depois de cada letra latina.
pub fn display_preview(preview: &str) -> String {
    preview.replace('\0', "")
}

/// Os campos que a busca enxerga, incluindo tipo e MIME quando não há texto.
pub fn search_fields(entry: &Entry, full_text: Option<&str>) -> Vec<String> {
    let mime = entry.mime.clone().unwrap_or_default();
    let mut fields = vec![display_preview(&entry.preview)];
    if entry.kind == IMAGE {
        let last = mime.rsplit('/').next().unwrap_or("").to_string();
        fields.extend(["imagem".into(), "image".into(), tr("image"), mime, last]);
    } else if entry.kind == BINARY {
        fields.extend(["binario".into(), "binário".into(), "binary".into(), tr("binary"), mime]);
    }
    if let Some(text) = full_text.filter(|t| !t.is_empty()) {
        fields.push(text.to_string());
    }
    fields
}

/// false quando a entrada já está em Fixados (para não repetir em Recentes). Com o sha conhecido, só o
/// conteúdo decide; sem ele, ID + prévia, até o índice terminar.
pub fn recent_visible(entry: &Entry, pins: &[Pin], sha: Option<&str>) -> bool {
    !pins.iter().any(|pin| match sha {
        Some(sha) => pin.sha == sha,
        None => pin.source_id == entry.id && pin.preview == entry.preview,
    })
}

// --- miniaturas ---

pub fn thumb_path(root: &Path, sha: &str) -> Result<PathBuf, ClipboardError> {
    if !is_sha(sha) {
        return Err(ClipboardError("invalid sha".into()));
    }
    Ok(root.join(format!("{sha}.png")))
}

/// Mantém as `keep` miniaturas mais novas e todas as dos fixados; apaga também os temporários de uma
/// gravação interrompida. Devolve quantas saíram.
pub fn prune_thumbs(root: &Path, keep: usize, keep_shas: &std::collections::HashSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else { return 0 };
    let mut files = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name.contains(".png.") {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if let Some(stem) = name.strip_suffix(".png").filter(|_| !name.starts_with('.')) {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                files.push((entry.path(), stem.to_string(), mtime));
            }
        }
    }
    let protected = files.iter().filter(|f| keep_shas.contains(&f.1)).count();
    let mut others: Vec<&(PathBuf, String, std::time::SystemTime)> = files.iter().filter(|f| !keep_shas.contains(&f.1)).collect();
    others.sort_by_key(|f| std::cmp::Reverse(f.2));
    let mut removed = 0;
    for (path, _, _) in others.into_iter().skip(keep.saturating_sub(protected)) {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn remove_thumb(root: &Path, sha: &str) {
    if let Ok(path) = thumb_path(root, sha) {
        let _ = std::fs::remove_file(path);
    }
}

/// Encaixa largura×altura em box×box sem estourar e sem chegar a zero (o `int()` do Python trunca).
pub fn scale_box(width: i32, height: i32, boxed: i32) -> (i32, i32) {
    if width <= 0 || height <= 0 {
        return (1, 1);
    }
    let factor = 1f64.min(f64::from(boxed) / f64::from(width)).min(f64::from(boxed) / f64::from(height));
    (((f64::from(width) * factor) as i32).max(1), ((f64::from(height) * factor) as i32).max(1))
}

/// Confirmação em dois cliques com prazo; o relógio é injetável nos testes.
#[derive(Debug)]
pub struct Confirm {
    pub timeout: Duration,
    armed_at: Option<Instant>,
}

impl Confirm {
    pub fn new(timeout: Duration) -> Confirm {
        Confirm { timeout, armed_at: None }
    }

    pub fn click(&mut self, now: Instant) -> bool {
        if self.armed_at.is_some_and(|at| now.duration_since(at) <= self.timeout) {
            self.reset();
            return true;
        }
        self.armed_at = Some(now);
        false
    }

    pub fn reset(&mut self) {
        self.armed_at = None;
    }
}

#[cfg(test)]
mod tests;
