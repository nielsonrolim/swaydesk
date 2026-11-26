//! O parser: os `bindsym`, `bindcode` e `bindgesture` do config, com as seções e as descrições dos
//! comentários `###` e `##`, seguindo os `include` e as variáveis do `set` como o Sway.
//!
//! O config e os comentários são entrada não confiável (o config.d é do usuário, e o /etc/sway/config.d de
//! qualquer pacote): a leitura tem limite de profundidade, de arquivos e de tamanho, e o texto só vai para
//! rótulos sem markup.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::glob;

/// Até onde os `include` descem (o config principal é o nível 0).
pub const MAX_DEPTH: usize = 8;
/// Quantos arquivos são lidos no total, contando o principal.
pub const MAX_FILES: usize = 256;
/// O maior arquivo lido; um maior fica de fora.
pub const MAX_BYTES: u64 = 1 << 20;
/// O maior valor de um `set`: cada `set` troca as variáveis na hora, e `set $b $a$a` dobra a cada linha.
pub const MAX_VALUE: usize = 64 << 10;
/// A maior linha depois de trocar as variáveis; uma maior fica de fora.
pub const MAX_LINE: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Sym,
    Code,
    Gesture,
}

/// Uma linha bind*: as teclas e o comando já com as variáveis do `set` trocadas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub kind: Kind,
    /// Como no config: "Mod4+Shift+e", "Mod4+49" ou "swipe:4:left".
    pub keys: String,
    pub command: String,
    /// --locked: vale também com a tela bloqueada.
    pub locked: bool,
    /// --release: dispara ao soltar. Conta na identidade: o Sway guarda os dois separados.
    pub release: bool,
}

/// Uma linha da lista: uma descrição e os binds que ela descreve (Super+1…9 numa só).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub section: Option<String>,
    /// O `mode "nome" { }` em que o bind está; None no modo padrão.
    pub mode: Option<String>,
    pub description: Option<String>,
    pub binds: Vec<Bind>,
}

/// O ambiente dos `include`: a HOME do ~ e as variáveis do $VAR.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: PathBuf,
    pub vars: HashMap<String, String>,
}

impl Env {
    pub fn from_process() -> Self {
        Env { home: swaydesk_common::config::home(), vars: std::env::vars().collect() }
    }

    pub fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).filter(|v| !v.is_empty()).cloned()
    }
}

/// Os atalhos do texto do config principal. `main` é o caminho dele: a pasta dos `include` relativos, e
/// não é lido de novo se algum `include` chegar a ele.
pub fn parse(text: &str, main: &Path, env: &Env) -> Vec<Entry> {
    let mut parser = Parser { env, vars: HashMap::new(), visited: HashSet::new(), files: 1, entries: Vec::new() };
    if let Ok(canonical) = main.canonicalize() {
        parser.visited.insert(canonical);
    }
    let base = main.parent().unwrap_or(Path::new("/")).to_path_buf();
    parser.text(text, &base, 0);
    drop_overridden(&mut parser.entries);
    join_same_description(&mut parser.entries);
    parser.entries
}

/// O arquivo como texto, se for um arquivo comum de até MAX_BYTES (UTF-8 inválido vira U+FFFD).
pub fn read_limited(path: &Path) -> Option<String> {
    // O stat antes do open: abrir um FIFO (um pipe com nome no config.d) travaria até alguém escrever nele.
    // O O_NONBLOCK cobre a troca do arquivo entre o stat e o open
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // O take segura um arquivo que cresceu depois do metadata
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

enum Block {
    Mode(String),
    /// `bindsym --locked { ... }`: cada linha de dentro é "teclas comando".
    Bind(Kind, Vec<String>),
    Other,
}

struct Parser<'a> {
    env: &'a Env,
    vars: HashMap<String, String>,
    visited: HashSet<PathBuf>,
    files: usize,
    entries: Vec<Entry>,
}

/// O estado de um arquivo: cada `include` começa sem seção e sem descrição pendente.
struct FileState {
    section: Option<String>,
    pending: Option<String>,
    /// A última linha útil foi um bind com descrição: o próximo bind entra na mesma linha da lista.
    open: Option<usize>,
    blocks: Vec<Block>,
}

impl Parser<'_> {
    fn text(&mut self, text: &str, base: &Path, depth: usize) {
        let mut st = FileState { section: None, pending: None, open: None, blocks: Vec::new() };
        for line in logical_lines(text) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                st.pending = None;
                st.open = None;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("###") {
                let title = rest.trim_start_matches('#').trim();
                st.section = (!title.is_empty()).then(|| title.to_string());
                st.pending = None;
                st.open = None;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("##") {
                let text = rest.trim();
                st.pending = (!text.is_empty()).then(|| text.to_string());
                st.open = None;
                continue;
            }
            if trimmed.starts_with('#') {
                continue; // um comentário comum no meio não corta a descrição
            }
            if trimmed == "}" {
                st.blocks.pop();
                st.pending = None;
                st.open = None;
                continue;
            }
            self.directive(trimmed, base, depth, &mut st);
        }
    }

    fn directive(&mut self, line: &str, base: &Path, depth: usize, st: &mut FileState) {
        if let Some(Block::Bind(kind, flags)) = st.blocks.last() {
            let (kind, flags) = (*kind, flags.clone());
            let Some(line) = self.substitute(line) else { return };
            let toks = tokens(&line);
            if let Some((keys, end)) = toks.first() {
                let command = line[*end..].trim();
                self.add_bind(st, kind, &flags, keys, command);
            }
            return;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if first == "set" {
            let toks = tokens(line);
            if let Some((name, end)) = toks.get(1) {
                if name.starts_with('$') && name.len() > 1 {
                    if let Some(value) = self.substitute(line[*end..].trim()).filter(|v| v.len() <= MAX_VALUE) {
                        self.vars.insert(name.clone(), unquote(&value));
                    }
                }
            }
            st.pending = None;
            st.open = None;
            return;
        }
        let Some(line) = self.substitute(line) else {
            st.pending = None;
            st.open = None;
            return;
        };
        let toks = tokens(&line);
        let Some((first, first_end)) = toks.first() else { return };
        if line.ends_with('{') {
            let head: Vec<&String> = toks.iter().map(|(t, _)| t).take_while(|t| t.as_str() != "{").collect();
            let block = match bind_kind(first) {
                Some(kind) => Block::Bind(
                    kind,
                    head.iter().skip(1).filter(|t| t.starts_with("--")).map(|t| t.to_string()).collect(),
                ),
                None if first == "mode" => match head.iter().skip(1).filter(|t| !t.starts_with("--")).last() {
                    Some(name) => Block::Mode(name.to_string()),
                    None => Block::Other,
                },
                None => Block::Other,
            };
            if !matches!(block, Block::Bind(..)) {
                st.pending = None;
            }
            st.open = None;
            st.blocks.push(block);
            return;
        }
        if first == "include" {
            st.pending = None;
            st.open = None;
            if depth < MAX_DEPTH {
                for path in self.expand(line[*first_end..].trim(), base) {
                    self.include(&path, depth + 1);
                }
            }
            return;
        }
        if let Some(kind) = bind_kind(first) {
            let mut flags = Vec::new();
            let mut rest = toks.iter().skip(1);
            for (tok, end) in rest.by_ref() {
                if tok.starts_with("--") {
                    flags.push(tok.clone());
                    continue;
                }
                let command = line[*end..].trim();
                self.add_bind(st, kind, &flags, tok, command);
                return;
            }
            return;
        }
        if let Some(kind) = first.strip_prefix("un").and_then(bind_kind) {
            if let Some((keys, _)) = toks.iter().skip(1).find(|(t, _)| !t.starts_with("--")) {
                let release = toks.iter().any(|(t, _)| t == "--release");
                let mode = current_mode(&st.blocks);
                let id = identity(mode.as_deref(), kind, release, keys);
                for entry in &mut self.entries {
                    let entry_mode = entry.mode.clone();
                    entry.binds.retain(|b| identity(entry_mode.as_deref(), b.kind, b.release, &b.keys) != id);
                }
            }
        }
        st.pending = None;
        st.open = None;
    }

    fn add_bind(&mut self, st: &mut FileState, kind: Kind, flags: &[String], keys: &str, command: &str) {
        if command.is_empty() {
            return;
        }
        let bind = Bind {
            kind,
            keys: keys.to_string(),
            command: command.to_string(),
            locked: flags.iter().any(|f| f == "--locked"),
            release: flags.iter().any(|f| f == "--release"),
        };
        if let Some(index) = st.open {
            self.entries[index].binds.push(bind);
            return;
        }
        self.entries.push(Entry {
            section: st.section.clone(),
            mode: current_mode(&st.blocks),
            description: st.pending.clone(),
            binds: vec![bind],
        });
        if st.pending.is_some() {
            st.open = Some(self.entries.len() - 1);
        }
    }

    fn include(&mut self, path: &Path, depth: usize) {
        if self.files >= MAX_FILES {
            return;
        }
        let Ok(canonical) = path.canonicalize() else { return };
        if !self.visited.insert(canonical.clone()) {
            return; // já lido: o Sway também não inclui duas vezes
        }
        let Some(text) = read_limited(&canonical) else { return };
        self.files += 1;
        let base = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        self.text(&text, &base, depth);
    }

    /// O caminho do `include` como o wordexp do Sway: ~, $VAR do ambiente, relativo à pasta do arquivo que
    /// inclui, e o glob, em ordem alfabética.
    fn expand(&self, raw: &str, base: &Path) -> Vec<PathBuf> {
        let raw = unquote(raw);
        let mut text = if raw == "~" {
            self.env.home.to_string_lossy().into_owned()
        } else if let Some(rest) = raw.strip_prefix("~/") {
            format!("{}/{rest}", self.env.home.to_string_lossy())
        } else {
            raw
        };
        text = env_substitute(&text, self.env);
        if text.is_empty() {
            return Vec::new();
        }
        let path = Path::new(&text);
        let path = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
        glob::expand(&path)
    }

    /// As variáveis do `set`, a mais longa primeiro, como o Sway: $mod2 não vira o valor de $mod seguido de 2.
    /// None se a linha passar de MAX_LINE (muitas variáveis grandes numa linha só).
    fn substitute(&self, line: &str) -> Option<String> {
        if self.vars.is_empty() || !line.contains('$') {
            return Some(line.to_string());
        }
        let mut names: Vec<&String> = self.vars.keys().collect();
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        let mut out = String::with_capacity(line.len());
        let mut rest = line;
        while let Some(pos) = rest.find('$') {
            out.push_str(&rest[..pos]);
            let tail = &rest[pos..];
            match names.iter().find(|n| tail.starts_with(n.as_str())) {
                Some(name) => {
                    if out.len() + self.vars[*name].len() > MAX_LINE {
                        return None;
                    }
                    out.push_str(&self.vars[*name]);
                    rest = &tail[name.len()..];
                }
                None => {
                    out.push('$');
                    rest = &tail[1..];
                }
            }
        }
        out.push_str(rest);
        Some(out)
    }
}

fn bind_kind(word: &str) -> Option<Kind> {
    match word {
        "bindsym" => Some(Kind::Sym),
        "bindcode" => Some(Kind::Code),
        "bindgesture" => Some(Kind::Gesture),
        _ => None,
    }
}

fn current_mode(blocks: &[Block]) -> Option<String> {
    blocks.iter().rev().find_map(|b| match b {
        Block::Mode(name) => Some(name.clone()),
        _ => None,
    })
}

/// As linhas com a barra invertida do fim juntando a seguinte, como o Sway.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        match line.strip_suffix('\\') {
            Some(start) => current.push_str(start),
            None => {
                current.push_str(line);
                out.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// As palavras da linha, sem as aspas, cada uma com o fim dela na linha (o comando é o resto depois das teclas).
fn tokens(line: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let inner = &line[start + 1..i.min(bytes.len())];
            i = (i + 1).min(bytes.len());
            out.push((inner.to_string(), i));
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push((line[start..i].to_string(), i));
        }
    }
    out
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    for q in ['"', '\''] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

/// $NOME e ${NOME} do ambiente; sem a variável, vazio, como o wordexp.
fn env_substitute(text: &str, env: &Env) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let rest = &text[i + 1..];
        let (name, used) = if let Some(inner) = rest.strip_prefix('{') {
            match inner.find('}') {
                Some(end) => (&inner[..end], end + 2),
                None => {
                    out.push('$');
                    continue;
                }
            }
        } else {
            let len = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
            (&rest[..len], len)
        };
        if name.is_empty() {
            out.push('$');
            continue;
        }
        out.push_str(&env.var(name).unwrap_or_default());
        for _ in 0..used {
            chars.next();
        }
    }
    out
}

/// O nome canônico de um modificador do Sway, ou None se não for um.
pub fn modifier(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "mod4" | "super" | "logo" => "Super",
        "mod1" | "alt" => "Alt",
        "ctrl" | "control" => "Ctrl",
        "shift" => "Shift",
        "mod5" => "Mod5",
        "mod3" => "Mod3",
        "mod2" => "Mod2",
        "lock" | "caps" => "Caps",
        _ => return None,
    })
}

/// O que o Sway compara para saber se dois binds são o mesmo: o modo, o tipo, o --release e as teclas,
/// com os modificadores em qualquer ordem e sem diferença de maiúsculas.
fn identity(mode: Option<&str>, kind: Kind, release: bool, keys: &str) -> (Option<String>, Kind, bool, String) {
    let normal = if kind == Kind::Gesture {
        keys.to_ascii_lowercase()
    } else {
        let mut mods = Vec::new();
        let mut rest = Vec::new();
        for part in keys.split('+') {
            match modifier(part) {
                Some(m) => mods.push(m.to_string()),
                None => rest.push(part.to_ascii_lowercase()),
            }
        }
        mods.sort();
        mods.dedup();
        mods.extend(rest);
        mods.join("+")
    };
    (mode.map(str::to_string), kind, release, normal)
}

/// A mesma descrição na mesma seção e no mesmo modo é uma linha só, com as teclas de todas, na posição da primeira:
/// o Super+F1 do keys/base.conf e o Super+/ do perfil, cada um sob o seu ##, como dois bindsym sob um ## só.
fn join_same_description(entries: &mut Vec<Entry>) {
    let mut first: HashMap<(Option<String>, Option<String>, String), usize> = HashMap::new();
    let mut joined: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries.drain(..) {
        let Some(description) = entry.description.clone() else {
            joined.push(entry);
            continue;
        };
        let key = (entry.section.clone(), entry.mode.clone(), description);
        match first.get(&key) {
            Some(&index) => joined[index].binds.extend(entry.binds),
            None => {
                first.insert(key, joined.len());
                joined.push(entry);
            }
        }
    }
    *entries = joined;
}

/// Um bind repetido mais adiante (o --no-warn do 50-local.conf) troca o de antes, como no Sway: o de
/// antes sai da lista, e uma linha que fica sem bind nenhum some.
fn drop_overridden(entries: &mut Vec<Entry>) {
    let mut seen = HashSet::new();
    for entry in entries.iter_mut().rev() {
        let mode = entry.mode.clone();
        let mut kept: Vec<Bind> = Vec::new();
        for bind in entry.binds.drain(..).rev() {
            if seen.insert(identity(mode.as_deref(), bind.kind, bind.release, &bind.keys)) {
                kept.push(bind);
            }
        }
        kept.reverse();
        entry.binds = kept;
    }
    entries.retain(|e| !e.binds.is_empty());
}
