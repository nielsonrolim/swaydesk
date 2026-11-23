//! A lógica do ícone e do popup "Sistema", sem GTK: o porte de `home/.local/bin/sway_system.py`.
//!
//! Tudo vem do /proc e do /sys, sem abrir processos. As funções recebem as raízes (`proc`, `sys`), e
//! [`proc_root`]/[`sys_root`] dão as de verdade, ou as de SWAY_DESKTOP_PROC e SWAY_DESKTOP_SYS.
mod paths;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use paths::{abspath, expanduser, home, realpath};

/// Quando o ícone fica em laranja: % da máquina, em duas leituras seguidas
pub const CPU_WARN: f64 = 90.0;
/// % da memória
pub const MEM_WARN: f64 = 90.0;
/// % do disco (só a barra no popup)
pub const DISK_WARN: f64 = 90.0;
/// °C, quando o sensor não dá o limite crítico
pub const TEMP_WARN: f64 = 90.0;

/// Os sensores da CPU, na ordem de preferência (hwmon e, sem eles, thermal_zone)
const HWMON_CPU: [&str; 4] = ["coretemp", "k10temp", "zenpower", "cpu_thermal"];
const ZONE_CPU: [&str; 3] = ["x86_pkg_temp", "cpu-thermal", "acpitz"];

/// A sessão: encerrar um destes derruba o desktop ou o deixa sem barra, som ou senhas (a lista do Python)
pub const PROTECTED: &[&str] = &[
    "sway", "swaybg", "swayidle", "swaylock", "Xwayland", "waybar", "dbus-daemon", "dbus-broker",
    "systemd", "pipewire", "pipewire-pulse", "wireplumber", "gnome-keyring-d", "xdg-desktop-por",
    "xdg-document-po", "xdg-permission-", "gvfsd", "at-spi-bus-laun", "at-spi2-registr", "polkit-gnome-au",
    "lxpolkit", "greetd", "nwg-hello",
    "sway-lock-gtk", "swaydesk-lock", "sway-lock-watch", "gtklock", "sway-gtklock",
    "sway-session", "(sd-pam)", "gdm-wayland-ses", "gdm-x-session", "polkit-mate-aut", "polkit-kde-auth",
    "wob", "swayrd", "wl-paste", "kwalletd5", "kwalletd6", "swaymsg",
];

/// O Resources é o do GNOME no Ubuntu 26.04, que não traz mais o gnome-system-monitor
pub const MONITORS: [&str; 5] = ["gnome-system-monitor", "mate-system-monitor", "plasma-systemmonitor", "resources", "missioncenter"];
/// Os binários em Rust: compilados e copiados para o ~/.local/bin pelo install.sh --rust, sem link para o
/// repositório
pub const RUST_PREFIX: &str = "swaydesk-";

/// Os shells: os laços do config são shells que o sway-after-env marca com
/// SWAY_DESKTOP_LOOP=<PID>:<starttime> antes do exec
pub const SHELLS: [&str; 4] = ["sh", "bash", "dash", "zsh"];
const LOOP_MARK: &str = "SWAY_DESKTOP_LOOP";
/// Onde ficam os programas do projeto, fora o ~/.local/bin: o sway-session e o sway-gpu
pub const SYSTEM_BIN: &str = "/usr/local/bin";
/// O system/usr/local/bin, sem o repositório à mão
pub const SYSTEM_NAMES: [&str; 2] = ["sway-session", "sway-gpu"];
/// Filhos de programas do projeto que podem ser encerrados mesmo assim: uma gravação travada
const ALLOW_CHILD: [&str; 1] = ["wf-recorder"];

pub fn proc_root() -> PathBuf {
    std::env::var_os("SWAY_DESKTOP_PROC").map(PathBuf::from).unwrap_or_else(|| "/proc".into())
}

pub fn sys_root() -> PathBuf {
    std::env::var_os("SWAY_DESKTOP_SYS").map(PathBuf::from).unwrap_or_else(|| "/sys".into())
}

fn read(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

// --- CPU ---

/// (ocupado, total) em jiffies, somando todos os núcleos. A espera de disco (iowait) conta como ocioso.
pub fn cpu_times(proc: &Path) -> std::io::Result<(u64, u64)> {
    let text = read(&proc.join("stat"))?;
    let mut fields: Vec<u64> = text.lines().next().unwrap_or("").split_whitespace().skip(1).take(8).filter_map(|v| v.parse().ok()).collect();
    fields.resize(8, 0);
    let (idle, iowait) = (fields[3], fields[4]);
    let total: u64 = fields.iter().sum();
    Ok((total - idle - iowait, total))
}

pub fn cpu_percent(before: (u64, u64), after: (u64, u64)) -> f64 {
    let busy = after.0 as f64 - before.0 as f64;
    let total = after.1 as f64 - before.1 as f64;
    if total > 0.0 {
        (100.0 * busy / total).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

pub fn load_average(proc: &Path) -> Option<[f64; 3]> {
    let text = read(&proc.join("loadavg")).ok()?;
    let values: Vec<f64> = text.split_whitespace().take(3).map(|v| v.parse().ok()).collect::<Option<_>>()?;
    values.try_into().ok()
}

// --- memória e disco ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

impl Memory {
    pub fn percent(&self) -> f64 {
        if self.total > 0 { 100.0 * self.used as f64 / self.total as f64 } else { 0.0 }
    }
}

/// Usada é o total menos a disponível (MemAvailable), como o free e os monitores fazem.
pub fn memory(proc: &Path) -> std::io::Result<Memory> {
    let mut values = HashMap::new();
    for line in read(&proc.join("meminfo"))?.lines() {
        let (key, rest) = line.split_once(':').unwrap_or((line, ""));
        if let Some(Ok(n)) = rest.split_whitespace().next().map(|v| v.parse::<u64>()) {
            values.insert(key.to_string(), n * 1024);
        }
    }
    let get = |k: &str| values.get(k).copied();
    let total = get("MemTotal").unwrap_or(0);
    let available = get("MemAvailable").or(get("MemFree")).unwrap_or(0);
    let swap_total = get("SwapTotal").unwrap_or(0);
    Ok(Memory {
        total,
        used: total.saturating_sub(available),
        swap_total,
        swap_used: swap_total.saturating_sub(get("SwapFree").unwrap_or(0)),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    /// O ponto de montagem
    pub path: String,
    pub used: u64,
    /// O que cabe para o usuário: usado mais o livre para ele, como o df
    pub total: u64,
}

impl Disk {
    pub fn percent(&self) -> f64 {
        if self.total > 0 { 100.0 * self.used as f64 / self.total as f64 } else { 0.0 }
    }
}

fn is_mount(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::symlink_metadata(path) else { return false };
    if meta.file_type().is_symlink() {
        return false;
    }
    let Some(parent) = path.parent() else { return true }; // a raiz
    let Ok(up) = std::fs::metadata(parent) else { return false };
    up.dev() != meta.dev() || up.ino() == meta.ino()
}

pub fn mount_point(path: &Path) -> PathBuf {
    let mut path = realpath(path);
    while !is_mount(&path) {
        match path.parent() {
            Some(parent) => path = parent.to_path_buf(),
            None => break,
        }
    }
    path
}

pub fn disk(path: &str) -> std::io::Result<Disk> {
    let c = std::ffi::CString::new(path).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    // SAFETY: statvfs só escreve na struct local; o caminho é um CString válido
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let frsize = st.f_frsize as u64;
    let used = (st.f_blocks as u64 - st.f_bfree as u64) * frsize;
    Ok(Disk { path: path.to_string(), used, total: used + st.f_bavail as u64 * frsize })
}

/// A raiz e, se for outra partição, a da pasta pessoal.
pub fn disks(home: &Path) -> Vec<Disk> {
    let mut points = vec!["/".to_string()];
    let home_point = mount_point(home);
    if home_point != Path::new("/") {
        points.push(home_point.to_string_lossy().into_owned());
    }
    points.iter().filter_map(|p| disk(p).ok()).collect()
}

// --- temperatura ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Temperature {
    pub celsius: f64,
    pub critical: Option<f64>,
}

impl Temperature {
    pub fn hot(&self) -> bool {
        self.celsius >= self.critical.unwrap_or(TEMP_WARN)
    }
}

fn milli(path: &Path) -> Option<f64> {
    read(path).ok()?.trim().parse::<i64>().ok().map(|v| v as f64 / 1000.0)
}

fn sorted_entries(dir: &Path) -> Option<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    Some(names)
}

/// A da CPU: a maior leitura do primeiro sensor conhecido, com o limite crítico dele. None sem sensor.
pub fn temperature(sys: &Path) -> Option<Temperature> {
    let hwmon = sys.join("class/hwmon");
    let mut by_name: HashMap<String, PathBuf> = HashMap::new();
    for entry in sorted_entries(&hwmon).unwrap_or_default() {
        if let Ok(name) = read(&hwmon.join(&entry).join("name")) {
            by_name.entry(name.trim().to_string()).or_insert_with(|| hwmon.join(&entry));
        }
    }
    for name in HWMON_CPU {
        let Some(base) = by_name.get(name) else { continue };
        let Some(files) = sorted_entries(base) else { continue }; // o sensor sumiu
        let (mut readings, mut limits) = (Vec::new(), Vec::new());
        for file in files.iter().filter(|f| f.starts_with("temp") && f.ends_with("_input")) {
            if let Some(value) = milli(&base.join(file)) {
                readings.push(value);
                if let Some(crit) = milli(&base.join(format!("{}_crit", &file[..file.len() - "_input".len()]))).filter(|c| *c != 0.0) {
                    limits.push(crit);
                }
            }
        }
        if !readings.is_empty() {
            let max = readings.iter().copied().fold(f64::MIN, f64::max);
            let crit = limits.iter().copied().reduce(f64::min);
            return Some(Temperature { celsius: max, critical: crit });
        }
    }
    let zones = sys.join("class/thermal");
    let mut types: HashMap<String, PathBuf> = HashMap::new();
    for zone in sorted_entries(&zones).unwrap_or_default().into_iter().filter(|z| z.starts_with("thermal_zone")) {
        if let Ok(kind) = read(&zones.join(&zone).join("type")) {
            types.entry(kind.trim().to_string()).or_insert_with(|| zones.join(&zone));
        }
    }
    for kind in ZONE_CPU {
        if let Some(value) = types.get(kind).and_then(|z| milli(&z.join("temp"))) {
            return Some(Temperature { celsius: value, critical: None });
        }
    }
    None
}

// --- processos ---

#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub pid: i32,
    /// O starttime do /proc: identifica o processo mesmo que o PID volte a ser usado
    pub start: u64,
    pub name: String,
    pub uid: i64,
    /// bytes
    pub rss: u64,
    /// utime + stime
    pub ticks: u64,
    /// A linha de comando
    pub args: Vec<String>,
    pub ppid: i32,
    /// A sessão (setsid): um app aberto numa sessão própria tem a dele
    pub sid: i32,
}

pub type Key = (i32, u64);

impl Process {
    pub fn key(&self) -> Key {
        (self.pid, self.start)
    }
}

fn page_size() -> u64 {
    // SAFETY: sysconf só lê uma configuração do sistema
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

fn stat_fields(stat: &str) -> Option<(String, Vec<&str>)> {
    let (open, close) = (stat.find('(')?, stat.rfind(')')?);
    let name = stat.get(open + 1..close)?.to_string();
    let fields = stat.get(close + 2..)?.split_whitespace().collect();
    Some((name, fields))
}

fn read_process(proc: &Path, pid: i32, page: u64) -> Option<Process> {
    let dir = proc.join(pid.to_string());
    let stat = read(&dir.join("stat")).ok()?;
    let (name, f) = stat_fields(&stat)?;
    // Depois do nome: estado (0), ppid (1)... utime (11), stime (12), starttime (19), rss (21)
    let field = |i: usize| f.get(i).and_then(|v| v.parse::<i64>().ok());
    let ppid = field(1)?;
    if ppid == 2 || pid == 2 {
        return None; // thread do kernel
    }
    let status = read(&dir.join("status")).ok()?;
    let uid = status
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    let args = read(&dir.join("cmdline")).map(|c| c.split('\0').filter(|a| !a.is_empty()).map(String::from).collect()).unwrap_or_default();
    Some(Process {
        pid,
        start: field(19)? as u64,
        name,
        uid,
        rss: field(21)? as u64 * page,
        ticks: (field(11)? + field(12)?) as u64,
        args,
        ppid: ppid as i32,
        sid: field(3)? as i32,
    })
}

/// Os processos de agora. Os que somem no meio da leitura ficam de fora.
pub fn processes(proc: &Path) -> HashMap<Key, Process> {
    let page = page_size();
    let Ok(entries) = std::fs::read_dir(proc) else { return HashMap::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(|pid| read_process(proc, pid, page))
        .map(|p| (p.key(), p))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Cpu,
    Memory,
}

/// [(processo, CPU %)]: os que mais usam CPU ou memória; `count` None dá todos, na ordem. A CPU % é da
/// máquina inteira: a diferença de tempo do processo sobre a do sistema.
pub fn top<'a>(
    before: &HashMap<Key, Process>,
    after: &'a HashMap<Key, Process>,
    cpu_before: (u64, u64),
    cpu_after: (u64, u64),
    key: Sort,
    count: Option<usize>,
) -> Vec<(&'a Process, f64)> {
    let elapsed = cpu_after.1 as f64 - cpu_before.1 as f64;
    let mut rows: Vec<(&Process, f64)> = after
        .iter()
        .map(|(k, p)| {
            let used = before.get(k).map(|old| p.ticks as f64 - old.ticks as f64).unwrap_or(0.0);
            (p, if elapsed > 0.0 { 100.0 * used / elapsed } else { 0.0 })
        })
        .collect();
    let by_float = |a: f64, b: f64| b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal);
    match key {
        Sort::Memory => rows.sort_by(|a, b| b.0.rss.cmp(&a.0.rss).then(by_float(a.1, b.1)).then(a.0.pid.cmp(&b.0.pid))),
        Sort::Cpu => rows.sort_by(|a, b| by_float(a.1, b.1).then(b.0.rss.cmp(&a.0.rss)).then(a.0.pid.cmp(&b.0.pid))),
    }
    if let Some(n) = count {
        rows.truncate(n);
    }
    rows
}

// --- o que pode ser encerrado ---

/// Como reconhecer um programa do projeto: pelo caminho real dentro do repositório (as instalações com
/// links) ou, em ~/.local/bin e /usr/local/bin, pelo nome de um programa do repositório (as cópias).
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub repo_dirs: HashSet<PathBuf>,
    pub names: HashSet<String>,
    pub install_dirs: HashSet<PathBuf>,
}

impl Project {
    pub fn owns(&self, path: &Path) -> bool {
        let real = realpath(path);
        if real.parent().is_some_and(|d| self.repo_dirs.contains(d)) {
            // Um arquivo de verdade: uma palavra qualquer de um sh -c resolvida numa pasta do projeto não conta
            return real.is_file();
        }
        let abs = abspath(path);
        abs.parent().is_some_and(|d| self.install_dirs.contains(d))
            && abs.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                self.names.contains(&*n) || n.starts_with(RUST_PREFIX)
            })
    }
}

fn listed(folder: &Path) -> Option<Vec<String>> {
    Some(std::fs::read_dir(folder).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with(['.', '_'])).collect())
}

/// O Project do repositório onde está `project_bin` (o home/.local/bin dele). Sem ele, ou sem o
/// repositório ao lado, vale o repositório do `~/.local/bin/sway_system.py`, um link que o install.sh
/// cria. Sem o repositório à mão, só os nomes do --system.
pub fn project(project_bin: Option<&Path>) -> Project {
    project_with_home(project_bin, &home())
}

/// O [`project`] com outra pasta pessoal (os testes).
pub fn project_with_home(project_bin: Option<&Path>, home: &Path) -> Project {
    let installed = || realpath(&home.join(".local/bin/sway_system.py")).parent().map(Path::to_path_buf).unwrap_or_default();
    let mut home_bin = realpath(&project_bin.map(Path::to_path_buf).unwrap_or_else(installed));
    let system_of = |bin: &Path| realpath(&bin.join("../../../system/usr/local/bin"));
    let mut system_bin = system_of(&home_bin);
    if !system_bin.is_dir() {
        home_bin = installed();
        system_bin = system_of(&home_bin);
    }
    let mut proj = Project::default();
    if system_bin.is_dir() {
        for folder in [&home_bin, &system_bin] {
            if let Some(names) = listed(folder) {
                proj.names.extend(names);
                proj.repo_dirs.insert(folder.clone());
            }
        }
    } else {
        proj.names.extend(SYSTEM_NAMES.iter().map(|s| s.to_string()));
    }
    for dir in [home.join(".local/bin"), PathBuf::from(SYSTEM_BIN)] {
        let abs = abspath(&dir);
        proj.install_dirs.insert(realpath(&abs));
        proj.install_dirs.insert(abs);
    }
    proj
}

fn is_interpreter(name: &str) -> bool {
    // ^(python[0-9.]*|bash|sh|dash|zsh)$
    matches!(name, "bash" | "sh" | "dash" | "zsh")
        || name.strip_prefix("python").is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Separa as palavras de um "sh -c '...'": espaços, pontuação do shell, $( ) e crases, chaves, aspas e o
/// "=" de VAR=valor.
fn split_words(arg: &str) -> impl Iterator<Item = &str> {
    arg.split(|c: char| c.is_whitespace() || ";&|()<>`{}\"'=".contains(c)).filter(|w| !w.is_empty())
}

fn expand_home(word: &str) -> String {
    let home = home();
    let home = home.to_string_lossy();
    word.replace("${HOME}", &home).replace("$HOME", &home)
}

/// Os caminhos que dizem qual programa roda: o argv[0] e, para um interpretador, as palavras dos
/// argumentos dele. O env é pulado, com as opções e os VAR=valor dele.
pub fn program_words(args: &[String]) -> Vec<String> {
    let mut args = args;
    while args.first().is_some_and(|a| basename(a) == "env") {
        args = &args[1..];
        while args.first().is_some_and(|a| a.starts_with('-') || a.contains('=')) {
            args = &args[1..];
        }
    }
    let Some(first) = args.first() else { return Vec::new() };
    let mut words = vec![first.clone()];
    if is_interpreter(basename(first)) {
        for arg in &args[1..] {
            words.extend(split_words(&expand_home(arg)).map(String::from));
        }
    }
    words
}

/// Resolve as palavras de um processo: ~ e $HOME expandidos, e os relativos a partir da pasta dele.
struct Resolver<'a> {
    proc: &'a Path,
    pid: i32,
    cwd: Option<Option<PathBuf>>,
}

impl Resolver<'_> {
    fn resolve(&mut self, word: &str) -> Option<PathBuf> {
        let word = expanduser(&expand_home(word));
        if word.is_absolute() {
            return Some(word);
        }
        let (proc, pid) = (self.proc, self.pid);
        let cwd = self.cwd.get_or_insert_with(|| std::fs::read_link(proc.join(pid.to_string()).join("cwd")).ok());
        cwd.as_ref().map(|c| c.join(word))
    }
}

fn project_exe(p: &Process, proj: &Project, proc: &Path) -> Option<PathBuf> {
    let exe = std::fs::read_link(proc.join(p.pid.to_string()).join("exe")).ok()?;
    // Um binário que o install trocou com ele rodando: o link termina em " (deleted)"
    let exe = match exe.to_str().and_then(|e| e.strip_suffix(" (deleted)")) {
        Some(e) => PathBuf::from(e),
        None => exe,
    };
    proj.owns(&exe).then_some(exe)
}

/// O caminho do programa do projeto que o processo roda, ou None: o executável, um programa da linha de
/// comando ou um que um interpretador ou shell roda.
pub fn project_program(p: &Process, proj: &Project, proc: &Path) -> Option<PathBuf> {
    if let Some(exe) = project_exe(p, proj, proc) {
        return Some(exe);
    }
    let mut paths = Resolver { proc, pid: p.pid, cwd: None };
    for (index, word) in program_words(&p.args).iter().enumerate() {
        if word.starts_with('-') || word == "." || word == ".." {
            continue;
        }
        if index == 0 && !word.contains('/') {
            continue; // um comando pelo PATH: o exe já foi conferido
        }
        if let Some(path) = paths.resolve(word).filter(|path| proj.owns(path)) {
            return Some(path);
        }
    }
    None
}

/// Se é um shell que o sway-after-env abriu: um laço do config, com a marca do próprio PID e starttime.
pub fn in_session_loop(p: &Process, proc: &Path) -> bool {
    if !SHELLS.contains(&p.name.as_str()) {
        return false;
    }
    let Ok(environ) = std::fs::read(proc.join(p.pid.to_string()).join("environ")) else { return false };
    let mark = format!("{LOOP_MARK}={}:{}", p.pid, p.start);
    environ.split(|b| *b == 0).any(|v| v == mark.as_bytes())
}

/// O programa do projeto que o processo roda diretamente, ou None: o executável, ou o script de um
/// interpretador. Um "sh -c '...'" que só menciona um programa não conta.
pub fn direct_program(p: &Process, proj: &Project, proc: &Path) -> Option<PathBuf> {
    if let Some(exe) = project_exe(p, proj, proc) {
        return Some(exe);
    }
    let first = p.args.first()?;
    if !is_interpreter(basename(first)) {
        return None;
    }
    let python = basename(first).starts_with("python");
    let value_flags: &[&str] = if python { &["-W", "-X", "--check-hash-based-pycs"] } else { &["-o", "-O", "--rcfile", "--init-file"] };
    let mut skip = false;
    for arg in &p.args[1..] {
        if skip {
            skip = false;
            continue;
        }
        if value_flags.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        if arg == "-" {
            return None; // código da entrada
        }
        if python && (arg.starts_with("-c") || arg.starts_with("-m")) {
            return None;
        }
        if python && (arg.starts_with("-W") || arg.starts_with("-X")) {
            continue; // o valor junto: -Wignore, -Xdev
        }
        let cluster = arg.len() > 1 && arg.starts_with('-') && arg[1..].chars().all(|c| c.is_ascii_alphabetic());
        if cluster && (if python { arg.contains('c') || arg.contains('m') } else { arg.contains('c') || arg.contains('s') }) {
            return None; // -Im, -ec, -s
        }
        if arg.starts_with('-') {
            continue;
        }
        let path = Resolver { proc, pid: p.pid, cwd: None }.resolve(arg)?;
        return proj.owns(&path).then_some(path);
    }
    None
}

/// Se o popup oferece "Encerrar": só processos do próprio usuário, fora da sessão e do projeto, e sem os
/// auxiliares dos programas do projeto na mesma sessão (o pactl do volume). `memo` guarda o programa de
/// cada pai já visto. Lê o /proc: rodar fora da linha da interface.
pub fn can_end(
    p: &Process,
    uid: i64,
    own_pid: i32,
    proj: &Project,
    proc: &Path,
    parent: Option<&Process>,
    memo: &mut HashMap<Key, bool>,
) -> bool {
    if p.uid != uid || p.pid == own_pid || PROTECTED.contains(&p.name.as_str()) {
        return false;
    }
    if project_program(p, proj, proc).is_some() || in_session_loop(p, proc) {
        return false;
    }
    if let Some(parent) = parent {
        if parent.uid == uid && parent.sid == p.sid && !ALLOW_CHILD.contains(&p.name.as_str()) {
            let direct = *memo.entry(parent.key()).or_insert_with(|| direct_program(parent, proj, proc).is_some());
            if direct {
                return false; // um auxiliar: o pactl do volume, o sleep do sway-session
            }
        }
    }
    true
}

/// O starttime do processo, ou None se ele não existe mais.
pub fn starttime(pid: i32, proc: &Path) -> Option<u64> {
    let stat = read(&proc.join(pid.to_string()).join("stat")).ok()?;
    stat_fields(&stat)?.1.get(19)?.parse().ok()
}

/// SIGTERM (ou SIGKILL com `force`), só se o PID ainda for o mesmo processo. Pelo pidfd: aberto antes de
/// conferir o starttime, ele continua apontando para o mesmo processo. true: o sinal foi.
pub fn end(p: &Process, force: bool, proc: &Path) -> bool {
    // SAFETY: syscalls com números; o fd é fechado no fim
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, p.pid, 0) } as i32;
    if fd < 0 {
        return false;
    }
    let sent = starttime(p.pid, proc) == Some(p.start) && {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: o fd é o pidfd aberto acima; sem siginfo
        unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, signal, std::ptr::null::<libc::siginfo_t>(), 0) == 0 }
    };
    // SAFETY: fecha o fd aberto acima
    unsafe { libc::close(fd) };
    sent
}

// --- o ícone ---

/// Se o ícone fica em laranja: a CPU alta nas duas últimas leituras, a memória quase cheia ou a
/// temperatura no limite.
pub fn warn_state(cpu_history: &[f64], mem: &Memory, temp: Option<&Temperature>) -> bool {
    let cpu_high = cpu_history.len() >= 2 && cpu_history[cpu_history.len() - 2..].iter().all(|v| *v >= CPU_WARN);
    cpu_high || mem.percent() >= MEM_WARN || temp.is_some_and(Temperature::hot)
}

/// O primeiro monitor do sistema instalado, ou None.
pub fn system_monitor() -> Option<&'static str> {
    let path = std::env::var_os("PATH")?;
    MONITORS.into_iter().find(|m| std::env::split_paths(&path).any(|d| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(d.join(m)).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }))
}

/// 1.5 GB, 830.0 MB; `decimal` formata o número (a vírgula do português).
pub fn size(value: u64, decimal: impl Fn(f64, usize) -> String) -> String {
    for (unit, scale) in [("TB", 1u64 << 40), ("GB", 1 << 30), ("MB", 1 << 20)] {
        if value >= scale {
            return format!("{} {unit}", decimal(value as f64 / scale as f64, 1));
        }
    }
    format!("{} kB", decimal(value as f64 / 1024.0, 0))
}

#[cfg(test)]
mod tests;
