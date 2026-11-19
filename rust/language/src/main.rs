//! swaydesk-language (waybar-language): uma linha da Waybar por mudança do layout ou dos teclados.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use swaydesk_language::{ipc, keyboards, line, registry, shown};

/// Onde o xkb guarda o registro dos layouts; o extras traz os layouts exóticos.
const RULES: &[&str] = &["/usr/share/X11/xkb/rules/evdev.xml", "/usr/share/X11/xkb/rules/evdev.extras.xml"];

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-language em Python confere que o binário carrega
    }
    let Some(socket) = std::env::var_os("SWAYSOCK").filter(|s| !s.is_empty()).map(PathBuf::from) else {
        println!("{}", line(None));
        eprintln!("waybar-language: SWAYSOCK is not set");
        return ExitCode::FAILURE;
    };
    let mut names = std::collections::HashMap::new();
    for path in RULES {
        if let Ok(xml) = std::fs::read_to_string(path) {
            for (full, short) in registry(&xml) {
                names.entry(full).or_insert(short);
            }
        }
    }
    match run(&socket, &names) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("waybar-language: {err}");
            ExitCode::FAILURE // a Waybar roda de novo depois do restart-interval
        }
    }
}

/// Assina os eventos antes de ler os inputs, para não perder uma mudança entre os dois; depois, cada evento
/// de input relê tudo, e só uma linha diferente da anterior sai.
fn run(socket: &Path, names: &std::collections::HashMap<String, String>) -> std::io::Result<()> {
    let mut events = ipc::subscribe_input(socket)?;
    let mut last = String::new();
    let mut out = std::io::stdout().lock();
    loop {
        let now = line(shown(&keyboards(&ipc::get_inputs(socket)?), names).as_ref());
        if now != last {
            writeln!(out, "{now}")?;
            out.flush()?;
            last = now;
        }
        loop {
            let (kind, _) = ipc::read(&mut events)?;
            if kind == ipc::EVENT_INPUT {
                break;
            }
        }
    }
}
