//! Popup "Sistema" para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-system-popup`,
//! que passa a vez a este binário (sway_rust). CPU, memória, disco, temperatura e os processos que mais
//! consomem, com a opção de encerrar um deles.
//!
//! Encerrar pede confirmação (um segundo clique em 3 s) e manda SIGTERM; se o processo continuar vivo,
//! o botão vira "Forçar" (SIGKILL). Só os processos do próprio usuário têm o botão, e nunca os da sessão
//! nem os do projeto. Com --monitor (clique direito no ícone), só abre o monitor do sistema.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use swaydesk_common::i18n::{decimal, tr};
use swaydesk_system_logic::{self as ss, Key, Process, Sort};
use swaydesk_ui::cmd::{bin, spawn_detached, status};
use swaydesk_ui::list::{link_button, set_status, Options, CSS_OPT};
use swaydesk_ui::{label, log, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-system-popup";
const REFRESH_S: u32 = 2;
const NAME_CHARS: i32 = 18;
const STATUS_CHARS: i32 = 30;
const DETAIL_CHARS: i32 = 52;
const ROWS: usize = 6;
/// Depois do SIGTERM, quanto esperar antes de oferecer "Forçar"
const STILL_ALIVE: Duration = Duration::from_secs(3);
/// O prazo do segundo clique
const CONFIRM: Duration = Duration::from_secs(3);
const WIDTH: i32 = 380;

const CSS: &str = r#"
levelbar trough { min-height: 6px; border-radius: 3px; background: #26262a; border: none; padding: 0; }
levelbar block.filled { border-radius: 3px; background: @accent; border: none; }
levelbar.warn block.filled { background: #f27835; }
levelbar block.empty { background: transparent; border: none; }
.value { font-size: 12px; }
.detail { color: #8a8a8f; font-size: 11px; }
.proc { font-size: 12px; }
.proc-num { font-size: 12px; color: #8a8a8f; }
button.end {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  padding: 1px 8px; border-radius: 6px; min-height: 0; font-size: 11px;
}
button.end:hover { background: #4c4c50; }
button.end.armed { background: #f27835; color: #1d1d20; }
button.end.force { background: #fc4138; color: #ffffff; }
.status { color: #f27835; font-size: 11px; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
"#;

fn size(value: u64) -> String {
    ss::size(value, decimal)
}

fn level_bar() -> gtk4::LevelBar {
    let bar = gtk4::LevelBar::for_interval(0.0, 100.0);
    bar.set_mode(gtk4::LevelBarMode::Continuous);
    for name in ["low", "high", "full"] {
        bar.remove_offset_value(Some(name)); // sem as cores padrão do tema
    }
    bar.set_hexpand(true);
    bar.set_valign(gtk4::Align::Center);
    bar
}

/// Uma linha com a barra e o número à direita, e um detalhe embaixo.
struct Meter {
    widget: gtk4::Box,
    bar: gtk4::LevelBar,
    value: gtk4::Label,
    detail: gtk4::Label,
}

impl Meter {
    fn new() -> Meter {
        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let bar = level_bar();
        let value = label("...", &["value"], 1.0); // até a primeira leitura; ASCII: a altura não muda
        value.set_width_chars(20); // "1023,9 GB de 1023,9 GB": a barra não muda de largura
        top.append(&bar);
        top.append(&value);
        widget.append(&top);
        let detail = label(" ", &["detail"], 0.0); // sempre com texto: o tamanho não muda
        detail.set_ellipsize(pango::EllipsizeMode::End);
        detail.set_width_chars(DETAIL_CHARS); // largura fixa, qualquer que seja o texto
        detail.set_max_width_chars(DETAIL_CHARS);
        widget.append(&detail);
        Meter { widget, bar, value, detail }
    }

    fn update(&self, percent: f64, value: &str, detail: &str, warn: bool) {
        self.bar.set_value(percent.clamp(0.0, 100.0));
        if warn {
            self.bar.add_css_class("warn");
        } else {
            self.bar.remove_css_class("warn");
        }
        self.value.set_text(value);
        self.detail.set_text(if detail.is_empty() { " " } else { detail });
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EndState {
    Idle,
    Armed,
    Ending,
    Force,
}

/// Nome, CPU %, memória e o botão de encerrar, que guarda o lugar mesmo escondido.
struct ProcessRow {
    widget: gtk4::Box,
    name: gtk4::Label,
    cpu: gtk4::Label,
    mem: gtk4::Label,
    button: gtk4::Button,
    proc: RefCell<Option<Process>>,
    offered: Cell<bool>,
}

impl ProcessRow {
    fn new() -> ProcessRow {
        let widget = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let name = label("", &["proc"], 0.0);
        name.set_ellipsize(pango::EllipsizeMode::End);
        name.set_width_chars(NAME_CHARS); // largura fixa: um nome longo não alarga o popup
        name.set_max_width_chars(NAME_CHARS);
        name.set_hexpand(true);
        let cpu = label("", &["proc-num"], 1.0);
        cpu.set_width_chars(6);
        let mem = label("", &["proc-num"], 1.0);
        mem.set_width_chars(9);
        let button = gtk4::Button::new();
        button.add_css_class("end");
        button.set_size_request(86, -1);
        for w in [name.upcast_ref::<gtk4::Widget>(), cpu.upcast_ref(), mem.upcast_ref(), button.upcast_ref()] {
            widget.append(w);
        }
        let row = ProcessRow { widget, name, cpu, mem, button, proc: RefCell::new(None), offered: Cell::new(false) };
        row.show_process(None, 0.0, None); // começa vazia, com o botão escondido
        row
    }

    /// Sem o botão, ele continua no lugar, transparente: o popup não muda de tamanho.
    fn offer(&self, on: bool) {
        self.offered.set(on);
        self.button.set_opacity(if on { 1.0 } else { 0.0 });
        self.button.set_sensitive(on);
        if !on {
            self.button.set_tooltip_text(None);
        }
    }

    fn show_process(&self, proc: Option<&Process>, cpu: f64, state: Option<EndState>) {
        self.proc.replace(proc.cloned());
        let Some(proc) = proc else {
            for l in [&self.name, &self.cpu, &self.mem] {
                l.set_text(" ");
            }
            self.name.set_tooltip_text(None);
            self.button.set_label(&tr("End"));
            self.offer(false);
            return;
        };
        self.name.set_text(&proc.name);
        self.name.set_tooltip_text(Some(&format!("{} ({})", proc.name, proc.pid)));
        self.cpu.set_text(&format!("{}%", decimal(cpu, 1)));
        self.mem.set_text(&size(proc.rss));
        let Some(state) = state else {
            self.button.set_label(&tr("End"));
            self.offer(false);
            return;
        };
        let (text, tip) = match state {
            EndState::Idle => (tr("End"), Some(tr("End the process (SIGTERM)"))),
            EndState::Armed => (tr("Confirm"), Some(tr("Click again to end it"))),
            EndState::Ending => (tr("Ending…"), None),
            EndState::Force => (tr("Force"), Some(tr("Still running: kill it (SIGKILL)"))),
        };
        self.button.set_label(&text);
        self.offer(true);
        self.button.set_sensitive(state != EndState::Ending);
        for (class, on) in [("armed", state == EndState::Armed), ("force", state == EndState::Force)] {
            if on {
                self.button.add_css_class(class);
            } else {
                self.button.remove_css_class(class);
            }
        }
        self.button.set_tooltip_text(tip.as_deref());
    }
}

/// Uma leitura, feita em outra linha.
struct Data {
    cpu: f64,
    load: Option<[f64; 3]>,
    temp: Option<ss::Temperature>,
    mem: ss::Memory,
    disks: HashMap<String, ss::Disk>,
    procs: HashMap<Key, Process>,
    before: HashMap<Key, Process>,
    times: ((u64, u64), (u64, u64)),
    endable: HashMap<Key, bool>,
}

type Verdicts = HashMap<(Key, Vec<String>, i32, Option<Key>, Option<Vec<String>>, Option<i32>), bool>;

/// O que a leitura precisa do popup: a amostra anterior, o cache e os processos que precisam do botão.
struct ReadInput {
    sample: Option<(HashMap<Key, Process>, (u64, u64))>,
    verdicts: Verdicts,
    keep: HashSet<Key>,
    project: Arc<ss::Project>,
}

fn read(input: ReadInput) -> Result<(Data, Verdicts), String> {
    let proc = ss::proc_root();
    let first = match input.sample {
        Some(sample) => sample,
        None => {
            // A primeira CPU % precisa de duas leituras
            let first = (ss::processes(&proc), ss::cpu_times(&proc).map_err(|e| e.to_string())?);
            std::thread::sleep(Duration::from_millis(300));
            first
        }
    };
    let (procs, times) = (ss::processes(&proc), ss::cpu_times(&proc).map_err(|e| e.to_string())?);
    let (endable, verdicts) = endable_of(&first.0, &procs, (first.1, times), &input.keep, input.verdicts, &input.project, &proc);
    let data = Data {
        cpu: ss::cpu_percent(first.1, times),
        load: ss::load_average(&proc),
        temp: ss::temperature(&ss::sys_root()),
        mem: ss::memory(&proc).map_err(|e| e.to_string())?,
        disks: ss::disks(&ss::home()).into_iter().map(|d| (d.path.clone(), d)).collect(),
        before: first.0,
        times: (first.1, times),
        endable,
        procs,
    };
    Ok((data, verdicts))
}

/// Quem pode ser encerrado, só entre os candidatos à lista: os 12 primeiros por CPU e por memória, e os
/// que precisam do botão. A resposta fica guardada pelo processo, pela linha de comando e pelo pai (com a
/// linha dele); os shells são conferidos sempre (a marca do laço vem do ambiente).
fn endable_of(
    before: &HashMap<Key, Process>,
    procs: &HashMap<Key, Process>,
    times: ((u64, u64), (u64, u64)),
    keep: &HashSet<Key>,
    known: Verdicts,
    project: &ss::Project,
    proc: &std::path::Path,
) -> (HashMap<Key, bool>, Verdicts) {
    let mut candidates: HashMap<Key, &Process> = HashMap::new();
    for sort in [Sort::Cpu, Sort::Memory] {
        for (p, _) in ss::top(before, procs, times.0, times.1, sort, Some(ROWS * 2)) {
            candidates.insert(p.key(), p);
        }
    }
    for k in keep {
        if let Some(p) = procs.get(k) {
            candidates.insert(*k, p);
        }
    }
    let by_pid: HashMap<i32, &Process> = procs.values().map(|p| (p.pid, p)).collect();
    // SAFETY: getuid e getpid não falham
    let (uid, own) = (unsafe { libc::getuid() } as i64, std::process::id() as i32);
    let (mut endable, mut verdicts, mut memo) = (HashMap::new(), Verdicts::new(), HashMap::new());
    for (k, p) in candidates {
        let parent = by_pid.get(&p.ppid).copied();
        let cache = (k, p.args.clone(), p.sid, parent.map(Process::key), parent.map(|q| q.args.clone()), parent.map(|q| q.sid));
        let verdict = match known.get(&cache) {
            Some(v) if !ss::SHELLS.contains(&p.name.as_str()) => *v,
            _ => ss::can_end(p, uid, own, project, proc, parent, &mut memo),
        };
        verdicts.insert(cache, verdict);
        endable.insert(k, verdict);
    }
    (endable, verdicts)
}

struct Popup {
    popup: Rc<PopupWindow>,
    /// Como reconhecer os programas do projeto: lido uma vez, usado na linha de leitura
    project: Arc<ss::Project>,
    cpu: Meter,
    mem: Meter,
    disks: Vec<(String, Meter)>,
    order: Options,
    sort: Cell<Sort>,
    rows: Vec<ProcessRow>,
    list_motion: gtk4::EventControllerMotion,
    status: gtk4::Label,
    reading: Cell<bool>,
    read_errors: Cell<u32>,
    read_error: RefCell<String>,
    sample: RefCell<Option<(HashMap<Key, Process>, (u64, u64))>>,
    verdicts: RefCell<Verdicts>,
    data: RefCell<Option<Data>>,
    /// Processo → quando o primeiro clique foi, enquanto espera o segundo
    confirms: RefCell<HashMap<Key, Instant>>,
    /// Processo → quando o SIGTERM foi, para oferecer "Forçar"
    ending: RefCell<HashMap<Key, Instant>>,
    reorder_pending: Cell<bool>,
    showing_rows: Cell<bool>,
    monitor: Option<&'static str>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-system", &format!("{CSS_OPT}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        card.add_css_class("card");
        card.set_size_request(WIDTH, -1);
        popup.window.set_child(Some(&card));

        card.append(&label(&tr("CPU"), &["section"], 0.0));
        let cpu = Meter::new();
        card.append(&cpu.widget);
        card.append(&label(&tr("Memory"), &["section"], 0.0));
        let mem = Meter::new();
        card.append(&mem.widget);
        card.append(&label(&tr("Disk"), &["section"], 0.0));
        let disk_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let disks: Vec<(String, Meter)> = ss::disks(&ss::home()).into_iter().map(|d| (d.path, Meter::new())).collect();
        for (_, meter) in &disks {
            disk_box.append(&meter.widget);
        }
        card.append(&disk_box);

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        head.append(&label(&tr("Processes"), &["section"], 0.0));
        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let me = me.clone();
            let order = Options::new(&[("cpu", tr("CPU")), ("memory", tr("Memory"))], move |key| {
                if let Some(p) = me.upgrade() {
                    p.on_sort(key);
                }
            });
            let status = label(" ", &["status"], 0.0);
            status.set_ellipsize(pango::EllipsizeMode::End);
            status.set_width_chars(STATUS_CHARS);
            status.set_max_width_chars(STATUS_CHARS);
            status.set_hexpand(true);
            Popup {
                popup: popup.clone(),
                project: Arc::new(ss::project(None)),
                cpu,
                mem,
                disks,
                order,
                sort: Cell::new(Sort::Cpu),
                rows: (0..ROWS).map(|_| ProcessRow::new()).collect(),
                list_motion: gtk4::EventControllerMotion::new(),
                status,
                reading: Cell::new(false),
                read_errors: Cell::new(0),
                read_error: RefCell::new(String::new()),
                sample: RefCell::new(None),
                verdicts: RefCell::new(Verdicts::new()),
                data: RefCell::new(None),
                confirms: RefCell::new(HashMap::new()),
                ending: RefCell::new(HashMap::new()),
                reorder_pending: Cell::new(false),
                showing_rows: Cell::new(false),
                monitor: ss::system_monitor(),
            }
        });
        this.order.widget.set_halign(gtk4::Align::End);
        this.order.widget.set_hexpand(true);
        this.order.select("cpu");
        head.append(&this.order.widget);
        card.append(&head);

        let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        for (i, row) in this.rows.iter().enumerate() {
            rows.append(&row.widget);
            let me = Rc::downgrade(&this);
            row.button.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    p.on_end_row(i);
                }
            });
        }
        // Com o mouse sobre a lista, as linhas não trocam de lugar: o botão debaixo do ponteiro continua
        // sendo do processo que o usuário viu. A saída reordena logo depois (num idle: mudar os botões
        // dentro do aviso geraria outros avisos)
        let me = Rc::downgrade(&this);
        this.list_motion.connect_leave(move |_| {
            if let Some(p) = me.upgrade() {
                p.reorder_soon();
            }
        });
        rows.add_controller(this.list_motion.clone());
        card.append(&rows);

        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        foot.append(&this.status);
        if let Some(monitor) = this.monitor {
            let me = Rc::downgrade(&this);
            foot.append(&link_button(&tr("System monitor"), &tr("Open %s").replace("%s", monitor), move || {
                if let Some(p) = me.upgrade() {
                    p.open_monitor();
                }
            }));
        }
        card.append(&foot);

        // A janela aparece já, com "..." nos números: a primeira leitura leva uns 0,3 s. O tamanho não muda
        // quando os números chegam: as colunas têm largura fixa
        this.popup.show();
        this.refresh();
        let me = Rc::downgrade(&this);
        glib::timeout_add_seconds_local(REFRESH_S, move || match me.upgrade() {
            Some(p) => {
                p.refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        this
    }

    // --- dados ---
    fn refresh(self: &Rc<Self>) {
        if self.reading.replace(true) {
            return;
        }
        // Os que precisam do botão mesmo fora dos primeiros: esperando o segundo clique, o "Forçar", ou à vista
        let mut keep: HashSet<Key> = self.ending.borrow().keys().chain(self.confirms.borrow().keys()).copied().collect();
        keep.extend(self.rows.iter().filter_map(|r| r.proc.borrow().as_ref().map(Process::key)));
        let input = ReadInput {
            sample: self.sample.borrow_mut().take(),
            verdicts: std::mem::take(&mut *self.verdicts.borrow_mut()),
            keep,
            project: self.project.clone(),
        };
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || read(input)).await;
            let Some(p) = me.upgrade() else { return };
            p.reading.set(false);
            match result {
                Ok(Ok((data, verdicts))) => {
                    p.verdicts.replace(verdicts);
                    p.apply(data);
                }
                Ok(Err(err)) => p.read_failed(&err),
                Err(_) => p.read_failed("panic"),
            }
        });
    }

    fn read_failed(&self, err: &str) {
        log(PROGRAM, &format!("leitura falhou: {err}"));
        self.read_errors.set(self.read_errors.get() + 1);
        if self.read_errors.get() == 1 {
            self.read_error.replace(tr("Could not read the system data"));
            self.set_status(&self.read_error.borrow());
        }
    }

    fn set_status(&self, text: &str) {
        self.popup.rendered_at.set(Instant::now()); // se o texto mudar a superfície, o focus-out não fecha
        set_status(&self.status, text.trim());
    }

    fn apply(self: &Rc<Self>, data: Data) {
        self.popup.rendered_at.set(Instant::now());
        if self.read_errors.replace(0) > 0 && self.status.text() == *self.read_error.borrow() {
            self.set_status(""); // só o aviso da leitura sai, não o de encerrar
        }
        self.sample.replace(Some((data.procs.clone(), data.times.1)));
        let mut detail = Vec::new();
        if let Some(load) = data.load {
            detail.push(tr("Load: %s").replace("%s", &load.iter().map(|v| decimal(*v, 2)).collect::<Vec<_>>().join(" · ")));
        }
        if let Some(temp) = &data.temp {
            detail.push(tr("%s °C").replace("%s", &decimal(temp.celsius, 0)));
        }
        let hot = data.temp.as_ref().is_some_and(ss::Temperature::hot);
        self.cpu.update(data.cpu, &format!("{}%", decimal(data.cpu, 0)), &detail.join("   "), data.cpu >= ss::CPU_WARN || hot);
        let mem = &data.mem;
        let of = |used: u64, total: u64| tr("%(used)s of %(total)s").replace("%(used)s", &size(used)).replace("%(total)s", &size(total));
        let swap = if mem.swap_total > 0 {
            tr("Swap: %(used)s of %(total)s").replace("%(used)s", &size(mem.swap_used)).replace("%(total)s", &size(mem.swap_total))
        } else {
            tr("No swap")
        };
        self.mem.update(mem.percent(), &of(mem.used, mem.total), &swap, mem.percent() >= ss::MEM_WARN);
        for (path, meter) in &self.disks {
            match data.disks.get(path) {
                Some(d) => meter.update(d.percent(), &of(d.used, d.total), path, d.percent() >= ss::DISK_WARN),
                None => meter.update(0.0, "--", path, false), // desmontado ou ilegível
            }
        }
        self.data.replace(Some(data));
        self.show_rows();
    }

    fn show_rows(&self) {
        if self.showing_rows.replace(true) {
            return; // um evento do GTK no meio da troca não começa outra
        }
        if let Some(data) = &*self.data.borrow() {
            self.fill_rows(data);
        }
        self.showing_rows.set(false);
    }

    fn fill_rows(&self, data: &Data) {
        let procs = &data.procs;
        // Os que já não existem saem das confirmações e da espera
        self.confirms.borrow_mut().retain(|k, _| procs.contains_key(k));
        self.ending.borrow_mut().retain(|k, _| procs.contains_key(k));
        let ranked = ss::top(&data.before, procs, data.times.0, data.times.1, self.sort.get(), None);
        let cpu_of: HashMap<Key, f64> = ranked.iter().map(|(p, c)| (p.key(), *c)).collect();
        // Fica na mesma linha quem espera o segundo clique ou o "Forçar" e, com o mouse sobre a lista,
        // todos: só os números mudam
        let holding = self.list_motion.contains_pointer();
        let mut pinned: HashMap<usize, &Process> = HashMap::new();
        for (i, row) in self.rows.iter().enumerate() {
            let Some(key) = row.proc.borrow().as_ref().map(Process::key) else { continue };
            if let Some(p) = procs.get(&key) {
                if holding || self.ending.borrow().contains_key(&key) || self.state(data, p) == Some(EndState::Armed) {
                    pinned.insert(i, p);
                }
            }
        }
        let pinned_keys: HashSet<Key> = pinned.values().map(|p| p.key()).collect();
        let mut rest = ranked.iter().filter(|(p, _)| !pinned_keys.contains(&p.key())).map(|(p, _)| *p);
        for (i, row) in self.rows.iter().enumerate() {
            match pinned.get(&i).copied().or_else(|| rest.next()) {
                None => row.show_process(None, 0.0, None),
                Some(p) => row.show_process(Some(p), cpu_of.get(&p.key()).copied().unwrap_or(0.0), self.state(data, p)),
            }
        }
    }

    fn state(&self, data: &Data, p: &Process) -> Option<EndState> {
        if !data.endable.get(&p.key()).copied().unwrap_or(false) {
            return None;
        }
        if let Some(at) = self.ending.borrow().get(&p.key()) {
            return Some(if at.elapsed() >= STILL_ALIVE { EndState::Force } else { EndState::Ending });
        }
        if self.confirms.borrow().get(&p.key()).is_some_and(|at| at.elapsed() <= CONFIRM) {
            return Some(EndState::Armed);
        }
        Some(EndState::Idle)
    }

    fn reorder_soon(self: &Rc<Self>) {
        if !self.reorder_pending.replace(true) {
            let me = Rc::downgrade(self);
            glib::idle_add_local_once(move || {
                if let Some(p) = me.upgrade() {
                    p.reorder_pending.set(false);
                    p.show_rows();
                }
            });
        }
    }

    // --- ações ---
    fn on_sort(&self, key: &str) {
        self.sort.set(if key == "memory" { Sort::Memory } else { Sort::Cpu });
        self.order.select(key);
        self.show_rows();
    }

    fn on_end_row(self: &Rc<Self>, index: usize) {
        let row = &self.rows[index];
        if !row.offered.get() {
            return;
        }
        let Some(proc) = row.proc.borrow().clone() else { return };
        let state = match &*self.data.borrow() {
            Some(data) => self.state(data, &proc),
            None => None,
        };
        let key = proc.key();
        let root = ss::proc_root();
        match state {
            Some(EndState::Force) => {
                if !ss::end(&proc, true, &root) {
                    self.set_status(&tr("Could not kill %s").replace("%s", &proc.name));
                }
            }
            Some(EndState::Idle | EndState::Armed) => {
                let armed = self.confirms.borrow().get(&key).is_some_and(|at| at.elapsed() <= CONFIRM);
                if armed {
                    self.confirms.borrow_mut().remove(&key);
                    if ss::end(&proc, false, &root) {
                        self.ending.borrow_mut().insert(key, Instant::now());
                        self.set_status("");
                    } else {
                        self.set_status(&tr("Could not end %s").replace("%s", &proc.name));
                    }
                } else {
                    self.confirms.borrow_mut().insert(key, Instant::now());
                    let me = Rc::downgrade(self);
                    glib::timeout_add_local_once(CONFIRM + Duration::from_millis(50), move || {
                        if let Some(p) = me.upgrade() {
                            p.show_rows();
                        }
                    });
                }
            }
            _ => {}
        }
        self.show_rows();
        if self.ending.borrow().contains_key(&key) {
            let me = Rc::downgrade(self);
            glib::timeout_add_local_once(STILL_ALIVE + Duration::from_millis(50), move || {
                if let Some(p) = me.upgrade() {
                    p.refresh();
                }
            });
        }
    }

    fn open_monitor(&self) {
        let Some(monitor) = self.monitor else { return };
        match spawn_detached(monitor, &[] as &[&str]) {
            Ok(()) => self.popup.close("monitor do sistema"),
            Err(err) => self.set_status(
                &tr("Could not open %(program)s: %(error)s").replace("%(program)s", monitor).replace("%(error)s", &err.to_string()),
            ),
        }
    }
}

/// --monitor (clique direito no ícone): só abre o monitor do sistema; sem nenhum, nada.
fn open_monitor_only() -> ExitCode {
    if let Some(monitor) = ss::system_monitor() {
        let _ = swaydesk_ui::cmd::spawn(monitor, &[] as &[&str]);
    }
    ExitCode::SUCCESS
}

fn notify_icon() {
    // O ícone esconde a dica com o popup aberto
    let _ = status(bin("waybar-notify"), &["waybar-system"]);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--check"] => ExitCode::SUCCESS, // o waybar-system-popup em Python confere que o binário carrega
        ["--monitor"] => open_monitor_only(),
        _ => run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon),
    }
}
