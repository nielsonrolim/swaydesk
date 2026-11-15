//! A lógica da dock, sem GTK: config, janelas do Sway, itens, fixados e quando aparecer.
//!
//! O porte do `sway_dock.py`, com o mesmo comportamento e os mesmos arquivos, para as duas versões
//! conviverem: os fixados em `~/.local/share/sway-dock/pinned`, a lista publicada para a grade em
//! `$XDG_RUNTIME_DIR/sway-dock.pinned-ids` e a fila de pedidos `sway-dock pin|unpin`.
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub use swaydesk_drawer_logic::dock::{icon_paths, pinned_ids_mtime, pinned_ids_path, read_pinned_ids, PIN_ACTIONS};

pub const MODES: &[&str] = &["auto-hide", "overlap", "always"];
pub const SCRATCHPAD: &str = "__i3_scratch";
pub const MIN_ICON: i64 = 16;
pub const MAX_ICON: i64 = 128;
pub const MAX_HIDE_DELAY: i64 = 10000;
pub const MAX_ANIMATION: i64 = 1000;
/// O menu leva um instante para aparecer na tela.
pub const MENU_GRACE_MS: u64 = 1000;
/// Com o botão apertado, o Sway manda os movimentos mesmo fora da dock.
pub const DRAG_IDLE_MS: u64 = 5000;

// --- Config ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: String,
    pub icon_size: i64,
    pub hide_delay_ms: i64,
    /// O comando do botão da grade. "sway-drawer" (o padrão): a grade de apps, achada como os outros
    /// componentes. Vazio: sem o botão
    pub launcher: String,
    pub ignore: Vec<String>,
    pub outputs: Vec<String>,
    pub animation_ms: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: "overlap".into(),
            icon_size: 40,
            hide_delay_ms: 400,
            launcher: "sway-drawer".into(),
            ignore: vec![],
            outputs: vec![],
            animation_ms: 200,
        }
    }
}

const CONFIG_KEYS: &[&str] = &["mode", "icon_size", "hide_delay_ms", "launcher", "ignore", "outputs", "animation_ms"];

fn int_in(value: &toml::Value, min: i64, max: i64) -> Option<i64> {
    value.as_integer().filter(|v| (min..=max).contains(v))
}

fn text_list(value: &toml::Value) -> Option<Vec<String>> {
    value.as_array()?.iter().map(|v| v.as_str().map(str::to_string)).collect()
}

/// Devolve a config e os avisos. Um valor ruim fica no padrão: a sessão não pode ficar sem dock.
pub fn parse_config(data: &toml::Table) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let mut warnings = Vec::new();
    let bad = |key: &str, warnings: &mut Vec<String>| {
        warnings.push(format!("config: invalid value in {key}; using the default"))
    };
    // Na ordem do Python, para os avisos saírem iguais
    if let Some(v) = data.get("mode") {
        match v.as_str().filter(|m| MODES.contains(m)) {
            Some(m) => config.mode = m.to_string(),
            None => bad("mode", &mut warnings),
        }
    }
    for (key, min, max) in [("icon_size", MIN_ICON, MAX_ICON), ("hide_delay_ms", 0, MAX_HIDE_DELAY),
                            ("animation_ms", 0, MAX_ANIMATION)] {
        if let Some(v) = data.get(key) {
            match int_in(v, min, max) {
                Some(n) => match key {
                    "icon_size" => config.icon_size = n,
                    "hide_delay_ms" => config.hide_delay_ms = n,
                    _ => config.animation_ms = n,
                },
                None => bad(key, &mut warnings),
            }
        }
    }
    if let Some(v) = data.get("launcher") {
        match v.as_str() {
            Some(s) => config.launcher = s.to_string(),
            None => bad("launcher", &mut warnings),
        }
    }
    for key in ["ignore", "outputs"] {
        if let Some(v) = data.get(key) {
            match text_list(v) {
                Some(list) if key == "ignore" => config.ignore = list,
                Some(list) => config.outputs = list,
                None => bad(key, &mut warnings),
            }
        }
    }
    for key in data.keys() {
        if !CONFIG_KEYS.contains(&key.as_str()) {
            warnings.push(format!("config: unknown option: {key}"));
        }
    }
    (config, warnings)
}

pub fn load_config(path: &Path) -> (Config, Vec<String>) {
    match fs::read_to_string(path) {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(table) => parse_config(&table),
            Err(err) => (Config::default(), vec![format!("config: could not read {}: {}", path.display(), err.message())]),
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => (Config::default(), vec![]),
        Err(err) => (Config::default(), vec![format!("config: could not read {}: {err}", path.display())]),
    }
}

// --- Janelas e saídas do Sway ---

/// x, y, largura, altura.
pub type Rect = (i64, i64, i64, i64);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Window {
    pub id: i64,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
    pub urgent: bool,
    pub workspace: String,
    pub output: String,
    pub rect: Rect,
    pub floating: bool,
}

fn text(node: &Value, key: &str) -> String {
    node.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn rect_of(node: &Value) -> Rect {
    let r = node.get("rect");
    let n = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_i64).unwrap_or(0);
    (n("x"), n("y"), n("width"), n("height"))
}

fn app_id(node: &Value) -> String {
    let id = text(node, "app_id");
    if !id.is_empty() {
        return id;
    }
    node.get("window_properties").and_then(|p| p.get("class")).and_then(Value::as_str).unwrap_or("").to_string()
}

fn children(node: &Value) -> impl Iterator<Item = &Value> {
    ["nodes", "floating_nodes"]
        .into_iter()
        .flat_map(move |k| node.get(k).and_then(Value::as_array).into_iter().flatten())
}

/// As janelas da árvore do Sway, sem as do scratchpad, por ordem de criação.
pub fn windows_from_tree(tree: &Value) -> Vec<Window> {
    fn walk(node: &Value, output: &str, workspace: &str, found: &mut Vec<Window>) {
        let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
        let output = if kind == "output" { text(node, "name") } else { output.to_string() };
        let workspace = if kind == "workspace" { text(node, "name") } else { workspace.to_string() };
        if workspace == SCRATCHPAD {
            return;
        }
        let has_children = children(node).next().is_some();
        let pid = node.get("pid").and_then(Value::as_i64).unwrap_or(0);
        if (kind == "con" || kind == "floating_con") && !has_children && pid != 0 {
            if let Some(id) = node.get("id").and_then(Value::as_i64) {
                found.push(Window {
                    id,
                    app_id: app_id(node),
                    title: text(node, "name"),
                    focused: node.get("focused").and_then(Value::as_bool).unwrap_or(false),
                    urgent: node.get("urgent").and_then(Value::as_bool).unwrap_or(false),
                    workspace: workspace.clone(),
                    output: output.clone(),
                    rect: rect_of(node),
                    floating: kind == "floating_con",
                });
            }
        }
        for child in children(node) {
            walk(child, &output, &workspace, found);
        }
    }
    let mut found = Vec::new();
    walk(tree, "", "", &mut found);
    found.sort_by_key(|w| w.id);
    found
}

/// Nome -> (retângulo, workspace visível) das saídas ativas.
pub fn outputs_from_tree(tree: &Value) -> HashMap<String, (Rect, Option<String>)> {
    let mut outputs = HashMap::new();
    for node in tree.get("nodes").and_then(Value::as_array).into_iter().flatten() {
        let name = text(node, "name");
        if node.get("type").and_then(Value::as_str) == Some("output") && name != "__i3" {
            let ws = node.get("current_workspace").and_then(Value::as_str).map(str::to_string);
            outputs.insert(name, (rect_of(node), ws));
        }
    }
    outputs
}

/// A saída do Sway com esta geometria: é o que há de comum com um monitor do GDK.
pub fn output_at(outputs: &HashMap<String, (Rect, Option<String>)>, geometry: Rect) -> Option<String> {
    let mut names: Vec<&String> = outputs.keys().collect();
    names.sort(); // o HashMap não tem ordem: com duas saídas no mesmo lugar (espelho), a mesma escolha sempre
    names.into_iter().find(|n| outputs[*n].0 == geometry).cloned()
}

pub fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Alguma janela visível nesta saída ocupa o espaço da dock?
pub fn overlapped(dock: Rect, windows: &[Window], output: &str, workspace: &str) -> bool {
    windows.iter().any(|w| w.output == output && w.workspace == workspace && rects_overlap(dock, w.rect))
}

/// Onde a dock fica no monitor: centralizada, colada na borda inferior.
pub fn dock_rect(monitor: Rect, width: i64, height: i64) -> Rect {
    let (x, y, mw, mh) = monitor;
    (x + (mw - width).div_euclid(2), y + mh - height, width, height)
}

pub fn should_show(mode: &str, hovering: bool, menu_open: bool, dragging: bool, is_overlapped: bool) -> bool {
    if mode == "always" || hovering || menu_open || dragging {
        return true;
    }
    mode == "overlap" && !is_overlapped
}

/// Arrastar ou redimensionar uma janela flutuante pode não gerar evento. Janelas lado a lado sempre
/// chegam até embaixo, então só as flutuantes visíveis mudam a resposta do overlap.
pub fn needs_polling(mode: &str, windows: &[Window], outputs: &HashMap<String, (Rect, Option<String>)>) -> bool {
    mode == "overlap"
        && windows.iter().any(|w| {
            w.floating && outputs.get(&w.output).and_then(|(_, ws)| ws.as_deref()) == Some(w.workspace.as_str())
        })
}

pub fn uses_hotspot(mode: &str) -> bool {
    mode != "always"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stuck {
    Menu,
    Drag,
}

/// O que ficou preso. Com um menu aberto ou um arrasto, a dock não esconde nem redesenha os itens. Se
/// o menu não chegou à tela, ou se o fim do arrasto se perdeu, ela ficava visível e com os ícones de
/// antes até ser reiniciada. menu_age_ms e drag_idle_ms são None sem menu ou sem arrasto.
pub fn stuck_state(menu_age_ms: Option<u64>, menu_mapped: bool, drag_idle_ms: Option<u64>, button1_down: bool) -> Option<Stuck> {
    if menu_age_ms.is_some_and(|age| age >= MENU_GRACE_MS) && !menu_mapped {
        return Some(Stuck::Menu);
    }
    if drag_idle_ms.is_some_and(|idle| !button1_down || idle >= DRAG_IDLE_MS) {
        return Some(Stuck::Drag);
    }
    None
}

// --- Animação ---

/// Do 0 ao 1 desacelerando no fim.
pub fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// O quanto a dock está escondida: 0 é visível, 1 é fora da tela. Interromper continua de onde parou:
/// o novo trecho parte do progresso atual.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slide {
    pub progress: f64,
    pub origin: f64,
    pub target: f64,
    pub started: i64,
    pub running: bool,
}

impl Slide {
    /// Vai para o alvo a partir do progresso atual. Devolve se há o que animar.
    pub fn start(&mut self, target: f64, now: i64) -> bool {
        self.origin = self.progress;
        self.target = target;
        self.started = now;
        self.running = self.progress != target;
        self.running
    }

    /// Avança até now (ms). Devolve se ainda está rodando.
    pub fn step(&mut self, now: i64, duration_ms: i64) -> bool {
        if !self.running {
            return false;
        }
        // A duração vale para o percurso inteiro: reverter com pouca distância termina rápido
        let span = duration_ms as f64 * (self.target - self.origin).abs();
        let elapsed = if span > 0.0 { ((now - self.started) as f64 / span).clamp(0.0, 1.0) } else { 1.0 };
        self.progress = self.origin + (self.target - self.origin) * ease_out(elapsed);
        if elapsed >= 1.0 {
            self.progress = self.target; // fecha exato, sem sobra de arredondamento
            self.running = false;
        }
        self.running
    }
}

// --- Apps e itens ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Sem .desktop.
    pub id: String,
    pub wm_class: String,
}

/// O id do app: exato, StartupWMClass, último trecho do id, depois sem o .py. None se não houver.
pub fn match_app(app_id: &str, entries: &[AppEntry]) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    let wanted = app_id.to_lowercase();
    let without_py = wanted.strip_suffix(".py").unwrap_or(&wanted).to_string();
    let tests: [&dyn Fn(&AppEntry) -> bool; 4] = [
        &|e| e.id.to_lowercase() == wanted,
        &|e| e.wm_class.to_lowercase() == wanted,
        &|e| e.id.to_lowercase().rsplit('.').next().unwrap_or("") == wanted,
        &|e| e.id.to_lowercase() == without_py,
    ];
    tests.iter().find_map(|test| entries.iter().find(|e| test(e)).map(|e| e.id.clone()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub pinned: bool,
    pub windows: Vec<Window>,
}

impl Item {
    pub fn focused(&self) -> bool {
        self.windows.iter().any(|w| w.focused)
    }

    pub fn urgent(&self) -> bool {
        self.windows.iter().any(|w| w.urgent)
    }
}

/// Fixados na ordem salva, depois os abertos não fixados, na ordem em que abriram.
pub fn build_items(pins: &[String], windows: &[Window], resolve: &dyn Fn(&str) -> Option<String>, ignore: &[String]) -> Vec<Item> {
    let ignored: HashSet<String> = ignore.iter().map(|n| n.to_lowercase()).collect();
    let mut items: Vec<Item> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for pin in pins {
        let key = resolve(pin).unwrap_or_else(|| pin.clone());
        if !ignored.contains(&key.to_lowercase()) && !ignored.contains(&pin.to_lowercase()) && !index.contains_key(&key) {
            index.insert(key.clone(), items.len());
            items.push(Item { key, pinned: true, windows: vec![] });
        }
    }
    for window in windows {
        let key = resolve(&window.app_id).unwrap_or_else(|| window.app_id.clone());
        if key.is_empty() || ignored.contains(&key.to_lowercase()) || ignored.contains(&window.app_id.to_lowercase()) {
            continue;
        }
        let at = *index.entry(key.clone()).or_insert_with(|| {
            items.push(Item { key, pinned: false, windows: vec![] });
            items.len() - 1
        });
        items[at].windows.push(window.clone());
    }
    items
}

// --- Fixados ---

/// Sem arquivo, não há fixados. Outro erro sobe: tratar como lista vazia apagaria os fixados ao salvar.
pub fn load_pins(path: &Path) -> io::Result<Vec<String>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(err) => return Err(err),
    };
    let mut seen = HashSet::new();
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && seen.insert(l.to_string()))
        .map(str::to_string)
        .collect())
}

/// Guarda ao lado um arquivo que não deu para ler, sem apagar cópias anteriores. Devolve o novo nome.
pub fn set_aside(path: &Path, now_secs: u64) -> io::Result<PathBuf> {
    let base = format!("{}.bad-{now_secs}", path.display());
    let mut target = PathBuf::from(&base);
    let mut count = 1;
    while target.exists() {
        target = PathBuf::from(format!("{base}-{count}"));
        count += 1;
    }
    fs::rename(path, &target)?;
    Ok(target)
}

fn lines(items: &[String]) -> Vec<u8> {
    items.iter().map(|i| format!("{i}\n")).collect::<String>().into_bytes()
}

pub fn save_pins(path: &Path, pins: &[String]) -> io::Result<()> {
    swaydesk_common::text::atomic_write(path, &lines(pins), 0o600)
}

/// ~/.local/share/sway-dock/pinned. Só a dock grava; a grade de apps lê, para o rótulo do menu.
pub fn pins_path() -> PathBuf {
    swaydesk_common::config::data_home().join("sway-dock").join("pinned")
}

pub fn pidfile(run: &Path) -> PathBuf {
    run.join("sway-dock.pid")
}

/// Se a dock está no ar: o PID do arquivo é de um processo sway-dock vivo.
pub fn dock_running(run: &Path) -> bool {
    swaydesk_common::resident::live_pid(&pidfile(run), "sway-dock").is_some()
}

/// A área da dock à vista, uma linha "x y largura altura" por monitor, em coordenadas do layout: a grade
/// de apps a deixa fora da área de cliques dela (o `holes` do sway-drawer).
pub fn area_path(run: &Path) -> PathBuf {
    run.join("sway-dock.area")
}

pub fn write_area(run: &Path, rects: &[Rect]) -> io::Result<()> {
    let text: Vec<String> = rects.iter().map(|(x, y, w, h)| format!("{x} {y} {w} {h}")).collect();
    swaydesk_common::text::atomic_write(&area_path(run), &lines(&text), 0o600)
}

pub fn remove_area(run: &Path) -> io::Result<()> {
    match fs::remove_file(area_path(run)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// O que a grade de apps cria enquanto está à vista (o `shown_path` do sway-drawer).
pub fn drawer_shown_path(run: &Path) -> PathBuf {
    run.join("sway-drawer.shown")
}

pub fn write_pinned_ids(run: &Path, ids: &[String]) -> io::Result<()> {
    swaydesk_common::text::atomic_write(&pinned_ids_path(run), &lines(ids), 0o600)
}

pub fn remove_pinned_ids(run: &Path) -> io::Result<()> {
    match fs::remove_file(pinned_ids_path(run)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

pub fn canonical_key(key: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    resolve(key).unwrap_or_else(|| key.to_string())
}

/// Se o app está nos fixados, comparando como a dock compara: "firefox" e "org.mozilla.firefox"
/// podem ser o mesmo app.
pub fn is_pinned(pins: &[String], key: &str, resolve: &dyn Fn(&str) -> Option<String>) -> bool {
    let wanted = canonical_key(key, resolve);
    pins.iter().any(|pin| canonical_key(pin, resolve) == wanted)
}

/// pin acrescenta no fim, se o app ainda não estiver; unpin tira todas as linhas que são o app.
pub fn apply_pin_request(pins: &[String], action: &str, key: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let wanted = canonical_key(key, resolve);
    if action == "pin" {
        if is_pinned(pins, key, resolve) {
            return pins.to_vec();
        }
        let mut out = pins.to_vec();
        out.push(wanted);
        return out;
    }
    pins.iter().filter(|pin| canonical_key(pin, resolve) != wanted).cloned().collect()
}

fn lock(file: &File) -> io::Result<()> {
    // SAFETY: o fd é válido enquanto o arquivo existe; a trava sai quando ele fecha
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn queue_request(path: &Path, action: &str, key: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    lock(&file)?;
    file.write_all(format!("{action}\t{key}\n").as_bytes())
}

/// Os pedidos da fila, na ordem, e a fila vazia. Linhas estranhas ficam de fora, e bytes que não são
/// UTF-8 viram U+FFFD (um id assim não casa com nenhum app). Outro erro de leitura sobe.
pub fn take_requests(path: &Path) -> io::Result<Vec<(String, String)>> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(err) => return Err(err),
    };
    lock(&file)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let (action, key) = line.split_once('\t')?;
            let key = key.trim();
            (PIN_ACTIONS.contains(&action) && !key.is_empty()).then(|| (action.to_string(), key.to_string()))
        })
        .collect())
}

/// `sway-dock pin|unpin ID`: enfileira e avisa a dock. true se ela recebeu o pedido. Sem a dock, nada é
/// gravado nem fica na fila (um pedido velho seria aplicado de surpresa depois): false.
pub fn pin_command(action: &str, key: &str, requests: &Path, run: &Path) -> io::Result<bool> {
    if !dock_running(run) {
        return Ok(false);
    }
    queue_request(requests, action, key)?;
    if swaydesk_common::resident::signal_pid(&pidfile(run), "sway-dock", libc::SIGHUP) {
        return Ok(true);
    }
    take_requests(requests)?; // a dock saiu agora
    Ok(false)
}

pub fn toggle_pin(pins: &[String], key: &str) -> Vec<String> {
    if pins.iter().any(|p| p == key) {
        pins.iter().filter(|p| *p != key).cloned().collect()
    } else {
        let mut out = pins.to_vec();
        out.push(key.to_string());
        out
    }
}

/// Coloca o fixado na posição dada; os outros mantêm a ordem.
pub fn move_pin(pins: &[String], key: &str, index: i64) -> Vec<String> {
    let mut rest: Vec<String> = pins.iter().filter(|p| *p != key).cloned().collect();
    let index = index.clamp(0, rest.len() as i64) as usize;
    rest.insert(index, key.to_string());
    rest
}

/// Em que posição fica um item solto na coordenada x, dadas as larguras dos itens em sequência.
pub fn drop_index(x: f64, widths: &[f64]) -> usize {
    let mut edge = 0.0;
    for (index, width) in widths.iter().enumerate() {
        if x < edge + width / 2.0 {
            return index;
        }
        edge += width;
    }
    widths.len().saturating_sub(1)
}
