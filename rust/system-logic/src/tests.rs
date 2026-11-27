//! Os testes de `tests/test_system.py`, com um /proc e um /sys de mentira numa pasta temporária.
use super::*;
use std::os::unix::fs::symlink;

const MEMINFO: &str = "MemTotal:        8000000 kB
MemFree:          500000 kB
MemAvailable:    2000000 kB
SwapTotal:       1000000 kB
SwapFree:         750000 kB
";

/// O `swaydesk_common::reference::repo_file`, aqui sem o swaydesk-common (este crate não depende dele): um
/// arquivo de referência do repositório, ou `None` com um aviso no tarball dos pacotes e no espelho, que não o
/// levam. No repositório completo (com o install.sh na raiz), um arquivo que sumiu é erro.
fn repo_file(rel: &str) -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(rel);
    if path.exists() {
        return Some(path);
    }
    assert!(!root.join("install.sh").exists(), "{rel}: reference file missing from the repository");
    // Direto no stderr, não pelo eprintln!: o libtest captura a saída dos testes que passam, e o aviso precisa
    // aparecer no log do build do pacote
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "skipped: the reference file {rel} is not in this source tree (the package tarball or the mirror)");
    None
}

fn stat_line(pid: i32, name: &str, ppid: i32, ticks: (u64, u64), start: u64, rss_pages: u64, sid: i32) -> String {
    format!("{pid} ({name}) S {ppid} {sid} {sid} 0 0 0 0 0 0 0 {} {} 0 0 20 0 1 0 {start} 0 {rss_pages}\n", ticks.0, ticks.1)
}

struct Fake {
    root: PathBuf,
    proc: PathBuf,
    sys: PathBuf,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone)]
struct P<'a> {
    name: &'a str,
    uid: i64,
    ppid: i32,
    ticks: (u64, u64),
    start: u64,
    rss_pages: u64,
    sid: i32,
    cmdline: Vec<String>,
}

fn p(name: &str) -> P<'_> {
    P { name, uid: 1000, ppid: 1, ticks: (0, 0), start: 100, rss_pages: 10, sid: 0, cmdline: vec![] }
}

impl<'a> P<'a> {
    fn uid(mut self, uid: i64) -> Self { self.uid = uid; self }
    fn ppid(mut self, ppid: i32) -> Self { self.ppid = ppid; self }
    fn ticks(mut self, t: u64) -> Self { self.ticks = (t, 0); self }
    fn start(mut self, s: u64) -> Self { self.start = s; self }
    fn rss(mut self, r: u64) -> Self { self.rss_pages = r; self }
    fn sid(mut self, s: i32) -> Self { self.sid = s; self }
    fn cmd(mut self, args: &[&str]) -> Self { self.cmdline = args.iter().map(|s| s.to_string()).collect(); self }
}

impl Fake {
    fn new() -> Fake {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("swaydesk-system-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (proc, sys) = (root.join("proc"), root.join("sys"));
        std::fs::create_dir_all(&proc).unwrap();
        let fake = Fake { root, proc, sys };
        fake.cpu(100, 1000);
        std::fs::write(fake.proc.join("meminfo"), MEMINFO).unwrap();
        std::fs::write(fake.proc.join("loadavg"), "0.52 0.61 0.70 1/300 1234\n").unwrap();
        fake
    }

    fn cpu(&self, busy: u64, idle: u64) {
        std::fs::write(self.proc.join("stat"), format!("cpu  {busy} 0 0 {idle} 0 0 0 0 0 0\ncpu0 1 0 0 1 0 0 0 0\n")).unwrap();
    }

    fn process(&self, pid: i32, spec: P) {
        let d = self.proc.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("stat"), stat_line(pid, spec.name, spec.ppid, spec.ticks, spec.start, spec.rss_pages, spec.sid)).unwrap();
        std::fs::write(d.join("status"), format!("Name:\t{}\nUid:\t{u}\t{u}\t{u}\t{u}\n", spec.name, u = spec.uid)).unwrap();
        let cmdline = if spec.cmdline.is_empty() { vec![spec.name.to_string()] } else { spec.cmdline };
        std::fs::write(d.join("cmdline"), cmdline.join("\0") + "\0").unwrap();
    }

    fn hwmon(&self, index: u32, name: &str, temps: &[f64], crit: Option<f64>) {
        let d = self.sys.join(format!("class/hwmon/hwmon{index}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        for (i, value) in temps.iter().enumerate() {
            std::fs::write(d.join(format!("temp{}_input", i + 1)), format!("{}\n", (value * 1000.0) as i64)).unwrap();
            if let Some(crit) = crit {
                std::fs::write(d.join(format!("temp{}_crit", i + 1)), format!("{}\n", (crit * 1000.0) as i64)).unwrap();
            }
        }
    }

    fn zone(&self, index: u32, kind: &str, value: f64) {
        let d = self.sys.join(format!("class/thermal/thermal_zone{index}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(d.join("temp"), format!("{}\n", (value * 1000.0) as i64)).unwrap();
    }

    fn by_pid(&self) -> HashMap<i32, Process> {
        processes(&self.proc).into_values().map(|p| (p.pid, p)).collect()
    }

    /// Um repositório falso: home/.local/bin com sway-dock, waybar-volume e sway-after-env,
    /// system/usr/local/bin com sway-session; e a pasta de instalação de mentira, com um link, uma cópia
    /// e um programa do usuário.
    fn project(&self, extra: &[&str]) -> (Project, PathBuf, PathBuf) {
        let repo = self.root.join("repo");
        let (home_bin, system_bin) = (repo.join("home/.local/bin"), repo.join("system/usr/local/bin"));
        for (folder, names) in [(&home_bin, vec!["sway-dock", "waybar-volume", "sway-after-env"]), (&system_bin, vec!["sway-session"])] {
            std::fs::create_dir_all(folder).unwrap();
            for name in names.iter().chain(if folder == &home_bin { extra.iter() } else { [].iter() }) {
                std::fs::write(folder.join(name), "#!/bin/sh\n").unwrap();
            }
        }
        let local = self.root.join("local-bin");
        std::fs::create_dir_all(&local).unwrap();
        let _ = symlink(home_bin.join("sway-dock"), local.join("sway-dock"));
        std::fs::write(local.join("waybar-volume"), "copiado").unwrap();
        std::fs::write(local.join("jupyter-lab"), "pip --user").unwrap();
        let mut proj = project(Some(&home_bin));
        proj.install_dirs = [realpath(&local)].into_iter().collect();
        (proj, home_bin, realpath(&local))
    }
}

fn can(fake: &Fake, procs: &HashMap<i32, Process>, pid: i32, proj: &Project, own: i32, with_parent: bool) -> bool {
    let p = &procs[&pid];
    let parent = if with_parent { procs.get(&p.ppid) } else { None };
    can_end(p, 1000, own, proj, &fake.proc, parent, &mut HashMap::new())
}

#[test]
fn cpu_percent_between_two_readings() {
    let fake = Fake::new();
    let before = cpu_times(&fake.proc).unwrap();
    fake.cpu(400, 1100); // +300 ocupados, +100 ociosos
    assert_eq!(cpu_percent(before, cpu_times(&fake.proc).unwrap()), 75.0);
    assert_eq!(cpu_percent((5, 10), (5, 10)), 0.0);
}

#[test]
fn load_and_memory() {
    let fake = Fake::new();
    assert_eq!(load_average(&fake.proc), Some([0.52, 0.61, 0.70]));
    let mem = memory(&fake.proc).unwrap();
    assert_eq!((mem.total, mem.used, mem.percent()), (8_000_000 * 1024, 6_000_000 * 1024, 75.0));
    assert_eq!((mem.swap_total, mem.swap_used), (1_000_000 * 1024, 250_000 * 1024));
    std::fs::write(fake.proc.join("meminfo"), "MemTotal: 1000 kB\nMemAvailable: 1000 kB\n").unwrap();
    let mem = memory(&fake.proc).unwrap();
    assert_eq!((mem.used, mem.swap_total, mem.swap_used), (0, 0, 0));
}

#[test]
fn sizes() {
    let plain = |n: f64, d: usize| format!("{n:.d$}");
    assert_eq!(size(1536 << 20, plain), "1.5 GB");
    assert_eq!(size(830 << 20, plain), "830.0 MB");
    assert_eq!(size(2048, plain), "2 kB");
    assert_eq!(size(1536 << 20, |n, d| format!("{n:.d$}").replace('.', ",")), "1,5 GB");
}

#[test]
fn temperatures() {
    let fake = Fake::new();
    fake.hwmon(0, "acpitz", &[40.0], None);
    fake.hwmon(1, "coretemp", &[55.0, 61.0, 58.0], Some(100.0));
    let temp = temperature(&fake.sys).unwrap();
    assert_eq!((temp.celsius, temp.critical), (61.0, Some(100.0)));
    assert!(!temp.hot());

    let fake = Fake::new();
    fake.hwmon(0, "BAT0", &[30.0], None);
    fake.zone(0, "acpitz", 47.0);
    fake.zone(1, "x86_pkg_temp", 66.0);
    assert_eq!(temperature(&fake.sys), Some(Temperature { celsius: 66.0, critical: None }));

    let fake = Fake::new();
    fake.hwmon(0, "coretemp", &[55.0], None);
    std::fs::remove_file(fake.sys.join("class/hwmon/hwmon0/temp1_input")).unwrap();
    fake.zone(0, "acpitz", 47.0);
    assert_eq!(temperature(&fake.sys), Some(Temperature { celsius: 47.0, critical: None }));

    assert_eq!(temperature(&Fake::new().sys), None);
    assert!(Temperature { celsius: 85.0, critical: Some(85.0) }.hot());
    assert!(!Temperature { celsius: 85.0, critical: None }.hot());
    assert!(Temperature { celsius: 90.0, critical: None }.hot());
}

fn process_fake() -> Fake {
    let fake = Fake::new();
    fake.process(10, p("firefox").ticks(100).rss(1000));
    fake.process(11, p("kitty").ticks(10).rss(5000));
    fake.process(12, p("kworker/0:1").ppid(2)); // thread do kernel
    fake.process(13, p("with) paren").ticks(1)); // o nome pode ter parênteses e espaços
    fake
}

#[test]
fn kernel_threads_are_left_out_and_odd_names_parse() {
    let fake = process_fake();
    let mut names: Vec<String> = processes(&fake.proc).into_values().map(|p| p.name).collect();
    names.sort();
    assert_eq!(names, ["firefox", "kitty", "with) paren"]);
}

#[test]
fn top_by_cpu_and_by_memory() {
    let fake = process_fake();
    let (before, cpu_before) = (processes(&fake.proc), cpu_times(&fake.proc).unwrap());
    fake.process(10, p("firefox").ticks(150).rss(1000)); // +50
    fake.process(11, p("kitty").ticks(30).rss(5000)); // +20
    fake.cpu(200, 1100); // +200 no total
    let (after, cpu_after) = (processes(&fake.proc), cpu_times(&fake.proc).unwrap());
    let by_cpu: Vec<(&str, f64)> = top(&before, &after, cpu_before, cpu_after, Sort::Cpu, Some(2)).into_iter().map(|(p, c)| (p.name.as_str(), c)).collect();
    assert_eq!(by_cpu, [("firefox", 25.0), ("kitty", 10.0)]);
    assert_eq!(top(&before, &after, cpu_before, cpu_after, Sort::Memory, Some(1))[0].0.name, "kitty");
}

#[test]
fn a_reused_pid_is_a_new_process() {
    let fake = process_fake();
    let before = processes(&fake.proc);
    fake.process(10, p("outro").ticks(500).start(999));
    let after = processes(&fake.proc);
    let rows = top(&before, &after, (0, 0), (0, 100), Sort::Cpu, None);
    assert_eq!(rows.iter().find(|(p, _)| p.name == "outro").unwrap().1, 0.0);
}

#[test]
fn who_can_be_ended() {
    let fake = process_fake();
    let (proj, _home_bin, local) = fake.project(&[]);
    let l = |n: &str| format!("{}/{n}", local.display());
    fake.process(20, p("sway").rss(1));
    fake.process(21, p("python3").cmd(&["/usr/bin/python3", "-W", "ignore", &l("sway-dock")]));
    fake.process(22, p("python3").cmd(&["/usr/bin/python3", "/tmp/meu-script.py"]));
    fake.process(23, p("cupsd").uid(0));
    fake.process(24, p("bash").cmd(&["/usr/bin/bash", "-o", "pipefail", &l("waybar-volume")]));
    fake.process(25, p("sh").cmd(&["sh", "-c", &format!("while :; do {}; sleep 1; done", l("sway-dock"))]));
    fake.process(26, p("sh").cmd(&["sh", "-c", "make"]));
    fake.process(27, p("python3").cmd(&["python3", &l("jupyter-lab")]));
    fake.process(28, p("vim").cmd(&["vim", &l("sway-dock")]));
    fake.process(29, p("shotwell").cmd(&["shotwell", "/tmp/foto.jpg"]));
    let procs = fake.by_pid();
    let expected = [(10, true), (11, false), (20, false), (21, false), (22, true), (23, false), (24, false), (25, false), (26, true), (27, true), (28, true), (29, true)];
    for (pid, want) in expected {
        assert_eq!(can(&fake, &procs, pid, &proj, 11, false), want, "{pid}");
    }
}

#[test]
fn words_inside_shell_syntax_and_env() {
    let fake = Fake::new();
    let (proj, home_bin, local) = fake.project(&[]);
    let l = |n: &str| format!("{}/{n}", local.display());
    fake.process(40, p("sh").cmd(&["sh", "-c", &format!("x=$({} get foo) && echo $x", l("sway-dock"))]));
    fake.process(41, p("sh").cmd(&["sh", "-c", &format!("FOO={}; `{}`", l("waybar-volume"), l("sway-dock"))]));
    fake.process(42, p("sh").cmd(&["env", "-i", "FOO=1", "python3", &l("sway-dock")]));
    fake.process(43, p("sh").cmd(&["sh", "-c", "cd sub && make; ls ."]));
    symlink(&home_bin, fake.proc.join("43/cwd")).unwrap();
    let procs = fake.by_pid();
    for (pid, want) in [(40, false), (41, false), (42, false), (43, true)] {
        assert_eq!(can(&fake, &procs, pid, &proj, 0, false), want, "{pid}");
    }
}

#[test]
fn the_real_executable_and_relative_programs_count() {
    let fake = Fake::new();
    let (proj, home_bin, local) = fake.project(&[]);
    fake.process(44, p("sway-dock").cmd(&["sway-dock"]));
    symlink(home_bin.join("sway-dock"), fake.proc.join("44/exe")).unwrap();
    fake.process(30, p("bash").cmd(&["bash", "waybar-volume"]));
    symlink(&local, fake.proc.join("30/cwd")).unwrap();
    let procs = fake.by_pid();
    assert!(!can(&fake, &procs, 44, &proj, 0, false));
    assert!(!can(&fake, &procs, 30, &proj, 0, false));
}

#[test]
fn the_rust_binaries_in_the_install_folder_are_the_project() {
    // Cópias compiladas, sem link para o repositório; um trocado com ele rodando vem com " (deleted)"
    let fake = Fake::new();
    let (proj, _, local) = fake.project(&[]);
    std::fs::write(local.join("swaydesk-notifd"), "ELF").unwrap();
    fake.process(90, p("sway-notifd").cmd(&["sway-notifd"]));
    symlink(local.join("swaydesk-notifd"), fake.proc.join("90/exe")).unwrap();
    fake.process(91, p("sway-drawer").cmd(&["sway-drawer"]));
    symlink(format!("{}/swaydesk-drawer (deleted)", local.display()), fake.proc.join("91/exe")).unwrap();
    // Um swaydesk-* fora das pastas de instalação não é do projeto
    fake.process(92, p("swaydesk-x").cmd(&["swaydesk-x"]));
    symlink("/tmp/swaydesk-x", fake.proc.join("92/exe")).unwrap();
    let procs = fake.by_pid();
    assert!(!can(&fake, &procs, 90, &proj, 0, false));
    assert!(!can(&fake, &procs, 91, &proj, 0, false));
    assert!(can(&fake, &procs, 92, &proj, 0, false));
}

#[test]
fn monitors_match_python() {
    let Some(path) = repo_file("home/.local/bin/sway_system.py") else { return };
    let python = std::fs::read_to_string(path).unwrap();
    let line = python.lines().find(|l| l.starts_with("MONITORS = (")).unwrap();
    let names: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    assert_eq!(names, MONITORS);
    assert!(python.contains(&format!("RUST_PREFIX = \"{RUST_PREFIX}\"")));
}

#[test]
fn session_helpers_seen_in_the_vms_are_protected() {
    let fake = Fake::new();
    let (proj, _, _) = fake.project(&[]);
    let names = ["sway-session", "(sd-pam)", "polkit-mate-aut", "swayrd", "swaymsg", "wl-paste"];
    for (i, name) in names.iter().enumerate() {
        fake.process(50 + i as i32, p(name));
    }
    let procs = fake.by_pid();
    for i in 0..names.len() as i32 {
        assert!(!can(&fake, &procs, 50 + i, &proj, 0, false), "{}", names[i as usize]);
    }
}

#[test]
fn helpers_of_project_programs_but_not_apps_they_open() {
    let fake = Fake::new();
    let (proj, home_bin, _) = fake.project(&["sway-shot", "sway-rec", "waybar-bluetooth-popup"]);
    let h = |n: &str| format!("{}/{n}", home_bin.display());
    fake.process(60, p("bash").cmd(&["bash", &h("waybar-volume")]));
    fake.process(61, p("pactl").ppid(60).cmd(&["pactl", "subscribe"]));
    fake.process(62, p("sh").cmd(&["sh", "-c", &format!("{} region edit", h("sway-shot"))]));
    fake.process(63, p("gradia").ppid(62).cmd(&["gradia", "/tmp/x.png"]));
    fake.process(64, p("python3").cmd(&["python3", &h("waybar-bluetooth-popup")]));
    fake.process(65, p("blueman-manage").ppid(64).sid(65).cmd(&["blueman-manager"]));
    fake.process(66, p("bash").cmd(&["bash", &h("sway-rec"), "full"]));
    fake.process(67, p("wf-recorder").ppid(66).cmd(&["wf-recorder", "-f", "x.mp4"]));
    fake.process(68, p("sway-session").cmd(&["/bin/sh", &h("waybar-volume")]));
    fake.process(69, p("sleep").ppid(68).cmd(&["sleep", "60"]));
    let procs = fake.by_pid();
    for (pid, want) in [(61, false), (63, true), (65, true), (67, true), (69, false)] {
        assert_eq!(can(&fake, &procs, pid, &proj, 0, true), want, "{pid}");
    }
}

#[test]
fn sway_session_children_through_the_install_folder() {
    let fake = Fake::new();
    let (mut proj, _, _) = fake.project(&[]);
    proj.install_dirs = [PathBuf::from("/usr/local/bin")].into_iter().collect();
    fake.process(80, p("sway-session").cmd(&["/bin/sh", "/usr/local/bin/sway-session"]));
    fake.process(81, p("sleep").ppid(80).cmd(&["sleep", "60"]));
    let procs = fake.by_pid();
    assert!(!can(&fake, &procs, 81, &proj, 0, true));
}

#[test]
fn dock_and_grid_apps_in_their_own_session_can_be_ended() {
    let fake = Fake::new();
    let (proj, home_bin, local) = fake.project(&["sway-drawer"]);
    fake.process(82, p("python3").cmd(&["python3", &format!("{}/sway-drawer", home_bin.display()), "start"]));
    fake.process(83, p("kitty").ppid(82).sid(83).cmd(&["kitty", "-e", "htop"]));
    fake.process(84, p("python3").cmd(&["python3", &format!("{}/sway-dock", local.display())]));
    fake.process(85, p("gimp").ppid(84).sid(85).cmd(&["gimp"]));
    let procs = fake.by_pid();
    assert!(can(&fake, &procs, 83, &proj, 0, true));
    assert!(can(&fake, &procs, 85, &proj, 0, true));
}

#[test]
fn without_the_repository_only_the_system_names() {
    let fake = Fake::new();
    let copied = fake.root.join("copied-bin");
    std::fs::create_dir_all(&copied).unwrap();
    std::fs::write(copied.join("jupyter-lab"), "# pip --user").unwrap();
    // Sem o repositório ao lado nem o link instalado (uma pasta pessoal vazia)
    let names = project_with_home(Some(&copied), &fake.root.join("no-home")).names;
    let want: HashSet<String> = SYSTEM_NAMES.iter().map(|s| s.to_string()).collect();
    assert_eq!(names, want);
}

#[test]
fn system_names_and_real_repository() {
    let repo_bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/bin");
    let system: HashSet<String> = std::fs::read_dir(repo_bin.join("../../../system/usr/local/bin")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(system, SYSTEM_NAMES.iter().map(|s| s.to_string()).collect());
    // O lançador do sway-dock: o tarball dos pacotes não leva os lançadores em Python
    let Some(_) = repo_file("home/.local/bin/sway-dock") else { return };
    let proj = project(Some(&repo_bin));
    assert!(proj.names.contains("sway-dock") && proj.names.contains("sway-session"));
}

#[test]
fn protected_names_match_python() {
    let Some(path) = repo_file("home/.local/bin/sway_system.py") else { return };
    let python = std::fs::read_to_string(path).unwrap();
    let block = python.split("PROTECTED = {").nth(1).unwrap().split("\n}").next().unwrap();
    let mut names: Vec<String> = block
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.split('"').skip(1).step_by(2).map(String::from).collect::<Vec<_>>())
        .collect();
    names.sort();
    let mut ours: Vec<String> = PROTECTED.iter().map(|s| s.to_string()).collect();
    ours.sort();
    assert_eq!(names, ours);
}

#[test]
fn direct_program_ignores_mentions_and_inline_code() {
    let fake = Fake::new();
    let (proj, _, local) = fake.project(&[]);
    let l = |n: &str| format!("{}/{n}", local.display());
    let (dock, vol) = (l("sway-dock"), l("waybar-volume"));
    let cases: Vec<(i32, Vec<&str>, bool)> = vec![
        (70, vec!["python3", "-W", "ignore", &dock], true),
        (71, vec!["bash", "-ec", &dock], false),
        (72, vec!["sh", "-c", &dock], false),
        (73, vec!["python3", "-m", "http.server"], false),
        (74, vec!["bash", "-o", "pipefail", &vol], true),
        (75, vec!["python3", "-O", &dock], true),
        (78, vec!["python3", "-Wignore::DeprecationWarning", &dock], true),
        (79, vec!["python3", "-mhttp.server"], false),
        (80, vec!["python3", "-Im", "http.server"], false),
        (76, vec!["bash", "-s", &vol], false),
        (77, vec!["python3", "-", &dock], false),
    ];
    for (pid, args, _) in &cases {
        fake.process(*pid, p(basename(args[0])).cmd(args));
    }
    let procs = fake.by_pid();
    for (pid, _, want) in &cases {
        assert_eq!(direct_program(&procs[pid], &proj, &fake.proc).is_some(), *want, "{pid}");
    }
}

#[test]
fn the_config_loop_is_protected_by_its_own_mark_and_its_children_are_not() {
    let fake = Fake::new();
    let (proj, _, _) = fake.project(&[]);
    fake.process(30, p("sh").start(555).cmd(&["sh", "-c", "while :; do waybar; done"]));
    std::fs::write(fake.proc.join("30/environ"), b"HOME=/h\0SWAY_DESKTOP_LOOP=30:555\0").unwrap();
    fake.process(31, p("bash").cmd(&["bash"]));
    std::fs::write(fake.proc.join("31/environ"), b"SWAY_DESKTOP_LOOP=30:555\0").unwrap();
    fake.process(32, p("sh").start(999).cmd(&["sh"]));
    std::fs::write(fake.proc.join("32/environ"), b"SWAY_DESKTOP_LOOP=32:555\0").unwrap();
    let procs = fake.by_pid();
    assert!(!can(&fake, &procs, 30, &proj, 0, false));
    assert!(can(&fake, &procs, 31, &proj, 0, false));
    assert!(can(&fake, &procs, 32, &proj, 0, false));
}

#[test]
fn warnings() {
    let low = Memory { total: 100, used: 10, swap_total: 0, swap_used: 0 };
    assert!(!warn_state(&[95.0], &low, None));
    assert!(!warn_state(&[40.0, 95.0], &low, None));
    assert!(warn_state(&[92.0, 95.0], &low, None));
    assert!(warn_state(&[], &Memory { used: 95, ..low }, None));
    assert!(warn_state(&[], &low, Some(&Temperature { celsius: 99.0, critical: None })));
}

// Com processos de verdade, criados pelo teste
fn spawn_sh(script: &str) -> (std::process::Child, Process) {
    let child = std::process::Command::new("sh").args(["-c", script]).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200)); // o trap instalado
    let info = processes(Path::new("/proc")).into_values().find(|p| p.pid == child.id() as i32).unwrap();
    (child, info)
}

#[test]
fn term_then_force() {
    use std::os::unix::process::ExitStatusExt;
    let (mut child, info) = spawn_sh("trap '' TERM; while :; do sleep 0.1; done");
    assert!(end(&info, false, Path::new("/proc")));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none()); // ignorou o SIGTERM
    assert!(end(&info, true, Path::new("/proc")));
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
}

#[test]
fn a_gone_or_moved_process_is_not_signalled() {
    let (mut child, info) = spawn_sh("sleep 30");
    let moved = Process { start: info.start + 1, ..info.clone() };
    assert!(!end(&moved, false, Path::new("/proc")));
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(!end(&info, false, Path::new("/proc")));
}

#[test]
fn python_like_paths() {
    assert_eq!(abspath(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    assert_eq!(realpath(Path::new("/nonexistent-xyz/a/../b")), PathBuf::from("/nonexistent-xyz/b"));
    assert!(is_interpreter("python3.12") && is_interpreter("python") && is_interpreter("sh"));
    assert!(!is_interpreter("python3-config") && !is_interpreter("fish"));
}

#[test]
fn the_footer_links_follow_what_is_installed() {
    assert_eq!(foot_links(None, false), []);
    assert_eq!(foot_links(Some("resources"), false), [FootLink::Monitor("resources")]);
    assert_eq!(foot_links(None, true), [FootLink::Settings]);
    // A Central fica por último, na ponta direita do rodapé
    assert_eq!(foot_links(Some("resources"), true), [FootLink::Monitor("resources"), FootLink::Settings]);
}
