//! Os fixados: `pins.json` e os blobs por sha256, independentes do histórico do cliphist. O porte do
//! `Pin` e do `PinStore` do `waybar_clipboard.py`, nos mesmos arquivos.
//!
//! Só uma leitura completa autoriza apagar órfãos ou regravar o `pins.json`: um item rejeitado ou um erro
//! passageiro não pode custar o conteúdo, que não existe em outro lugar.
//!
//! Limite conhecido (o mesmo do Python): com duas instâncias ao mesmo tempo, a faxina de uma pode apagar o
//! blob que a outra acabou de gravar, antes de ela salvar o `pins.json`. Um popup por vez evita isso.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use swaydesk_common::i18n::tr;
use swaydesk_common::text::atomic_write;

use crate::{classify, is_sha, sha256, ClipboardError, Entry, KINDS, TEXT};

pub const PIN_VERSION: u64 = 1;

/// Uma cópia independente de um item, guardada fora do cliphist.
#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub sha: String,
    pub kind: String,
    pub mime: Option<String>,
    pub preview: String,
    pub source_id: String,
    pub pinned_at: f64,
    pub size: u64,
}

impl Pin {
    pub fn to_json(&self) -> Value {
        json!({
            "sha": self.sha, "kind": self.kind, "mime": self.mime, "preview": self.preview,
            "source_id": self.source_id, "pinned_at": self.pinned_at, "size": self.size,
        })
    }

    /// Um item do `pins.json`, conferido campo a campo como no `Pin.from_dict`.
    pub fn from_json(raw: &Value) -> Result<Pin, String> {
        let obj = raw.as_object().ok_or("pin is not an object")?;
        let sha = obj.get("sha").and_then(Value::as_str).filter(|s| is_sha(s)).ok_or("invalid sha")?;
        let mut kind = obj.get("kind").and_then(Value::as_str).filter(|k| KINDS.contains(k)).ok_or("invalid kind")?.to_string();
        let mut mime = match obj.get("mime") {
            None | Some(Value::Null) => None,
            Some(Value::String(m)) => Some(m.clone()),
            _ => return Err("invalid mime".into()),
        };
        let text = |key: &str, default: Option<&str>| match obj.get(key) {
            Some(Value::String(v)) => Ok(v.clone()),
            None => default.map(String::from).ok_or_else(|| "invalid preview or source".to_string()),
            _ => Err("invalid preview or source".to_string()),
        };
        let preview = text("preview", Some(""))?;
        let source_id = text("source_id", Some(""))?;
        let pinned_at = obj.get("pinned_at").and_then(Value::as_f64).filter(|_| obj.get("pinned_at").is_some_and(Value::is_number)).ok_or("invalid date")?;
        let size = obj.get("size").and_then(Value::as_u64).ok_or("invalid size")?;
        if kind == TEXT && mime.is_none() {
            // Fixado antes de o cliphist 0.5 ser reconhecido
            let (k, m) = classify(&preview);
            if k != TEXT && m.is_some() && preview.starts_with("[[ binary data ") {
                kind = k.to_string();
                mime = m;
            }
        }
        Ok(Pin { sha: sha.into(), kind, mime, preview, source_id, pinned_at, size })
    }
}

pub struct PinStore {
    pub root: PathBuf,
    pub pins_dir: PathBuf,
    pub meta_path: PathBuf,
    /// Se a última leitura foi completa (só ela autoriza regravar e apagar órfãos)
    pub last_read_ok: bool,
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl PinStore {
    pub fn new(root: &Path) -> PinStore {
        PinStore { root: root.into(), pins_dir: root.join("pins"), meta_path: root.join("pins.json"), last_read_ok: true }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.root, &self.pins_dir] {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    pub fn blob_path(&self, sha: &str) -> Result<PathBuf, ClipboardError> {
        if !is_sha(sha) {
            return Err(ClipboardError("invalid sha".into()));
        }
        Ok(self.pins_dir.join(format!("{sha}.bin")))
    }

    /// Os itens crus e se a leitura foi completa.
    fn read_raw(&self) -> (Vec<Value>, bool) {
        let Ok(text) = std::fs::read_to_string(&self.meta_path) else { return (Vec::new(), false) };
        let Ok(raw) = serde_json::from_str::<Value>(&text) else { return (Vec::new(), false) };
        match raw {
            Value::Array(items) => (items, true),
            // Um objeto sem `items` ou de outra versão não é uma lista vazia de fixados
            // 1 e 1.0 são a mesma versão, como no Python
            Value::Object(obj) if obj.get("version").and_then(Value::as_f64) == Some(PIN_VERSION as f64) => match obj.get("items") {
                Some(Value::Array(items)) => (items.clone(), true),
                _ => (Vec::new(), false),
            },
            _ => (Vec::new(), false),
        }
    }

    /// Os fixados válidos, do mais novo ao mais antigo; `last_read_ok` diz se a leitura foi completa.
    pub fn load(&mut self) -> Vec<Pin> {
        let (raw, mut complete) = self.read_raw();
        let mut pins = Vec::new();
        for item in &raw {
            let Ok(pin) = Pin::from_json(item) else {
                complete = false;
                continue;
            };
            let Ok(path) = self.blob_path(&pin.sha) else { continue };
            match std::fs::read(&path) {
                Ok(data) if sha256(&data) == pin.sha => pins.push(pin),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => complete = false,
            }
        }
        self.last_read_ok = complete;
        if complete {
            let valid: std::collections::HashSet<&str> = pins.iter().map(|p| p.sha.as_str()).collect();
            if let Ok(entries) = std::fs::read_dir(&self.pins_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    // O glob("*.bin") do Python pula os ocultos
                    if let Some(stem) = name.strip_suffix(".bin").filter(|_| !name.starts_with('.')) {
                        if !valid.contains(stem) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
        pins.sort_by(|a, b| b.pinned_at.partial_cmp(&a.pinned_at).unwrap_or(std::cmp::Ordering::Equal));
        pins
    }

    pub fn save(&self, pins: &[Pin]) -> Result<(), ClipboardError> {
        self.ensure().map_err(|e| ClipboardError(e.to_string()))?;
        let payload = json!({"version": PIN_VERSION, "items": pins.iter().map(Pin::to_json).collect::<Vec<_>>()});
        let text = serde_json::to_string_pretty(&payload).map_err(|e| ClipboardError(e.to_string()))?;
        atomic_write(&self.meta_path, text.as_bytes(), 0o600).map_err(|e| ClipboardError(e.to_string()))
    }

    fn refuse_if_unreadable(&self) -> Result<(), ClipboardError> {
        if self.meta_path.exists() && !self.last_read_ok {
            return Err(ClipboardError(tr("The pinned items file is unreadable; it will not be overwritten.")));
        }
        Ok(())
    }

    /// Fixa o conteúdo; o mesmo conteúdo de novo só atualiza a data e sobe para o topo.
    pub fn pin(&mut self, entry: &Entry, data: &[u8], when: Option<f64>) -> Result<Pin, ClipboardError> {
        if data.is_empty() {
            return Err(ClipboardError(tr("The item is empty.")));
        }
        self.ensure().map_err(|e| ClipboardError(e.to_string()))?;
        let when = when.unwrap_or_else(now);
        let digest = sha256(data);
        let mut pins = self.load();
        self.refuse_if_unreadable()?;
        let current = Pin {
            sha: digest.clone(),
            kind: entry.kind.clone(),
            mime: entry.mime.clone(),
            preview: entry.preview.clone(),
            source_id: entry.id.clone(),
            pinned_at: when,
            size: data.len() as u64,
        };
        if pins.iter().any(|p| p.sha == digest) {
            pins.retain(|p| p.sha != digest);
        } else {
            atomic_write(&self.blob_path(&digest)?, data, 0o600).map_err(|e| ClipboardError(e.to_string()))?;
        }
        pins.push(current.clone());
        pins.sort_by(|a, b| b.pinned_at.partial_cmp(&a.pinned_at).unwrap_or(std::cmp::Ordering::Equal));
        self.save(&pins)?;
        Ok(current)
    }

    /// Tira dos fixados e apaga o blob. Devolve se havia.
    pub fn unpin(&mut self, sha: &str) -> Result<bool, ClipboardError> {
        let pins = self.load();
        self.refuse_if_unreadable()?;
        let remaining: Vec<Pin> = pins.iter().filter(|p| p.sha != sha).cloned().collect();
        if remaining.len() == pins.len() {
            return Ok(false);
        }
        self.save(&remaining)?;
        if let Ok(path) = self.blob_path(sha) {
            let _ = std::fs::remove_file(path);
        }
        Ok(true)
    }
}
