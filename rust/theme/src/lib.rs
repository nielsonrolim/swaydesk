//! Os temas GTK instalados e a troca de tema em todo o desktop, sem GTK: o porte do `sway_themes.py`, do
//! `sway_theme_families.py`, do `sway_papirus.py` e da parte do `desktop_config.py` que grava os arquivos.
//! O `sway-theme`, o `sway-desktop-config` e o app Tema (o sway-theme-app) usam esta biblioteca.
pub mod desktop;
pub mod families;
pub mod icon_themes;
pub mod local_conf;
pub mod papirus;
pub mod setup;
pub mod themes;
pub mod wallpaper_accent;

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Um erro que o CLI mostra e encerra com 2: o ValueError e o OSError do Python.
#[derive(Debug)]
pub enum Error {
    Value(String),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Value(text) => f.write_str(text),
            Error::Io(err) => write!(f, "{err}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

/// O comando do tema que as mensagens mandam digitar: "sway-theme" nos dotfiles (o lançador em Python, ou o link
/// do --rust-only, fica ao lado do binário em ~/.local/bin) e "swaydesk-theme" nos pacotes (no /usr/bin não há
/// sway-theme; o do libexec é um link que o current_exe já resolve para o /usr/bin).
pub fn command_name() -> &'static str {
    static NAME: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    NAME.get_or_init(|| command_beside(std::env::current_exe().ok().as_deref().and_then(Path::parent)))
}

/// O command_name de um binário nesta pasta.
pub fn command_beside(dir: Option<&Path>) -> &'static str {
    if dir.is_some_and(|d| d.join("sway-theme").exists()) {
        "sway-theme"
    } else {
        "swaydesk-theme"
    }
}

/// O texto traduzido com os `%(nome)s` trocados, como o `_("...") % {...}` do Python.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (key, value) in values {
        text = text.replace(&format!("%({key})s"), value);
    }
    text
}

/// O `_("...%s...") % valor` do Python.
pub fn fill_one(template: &str, value: &str) -> String {
    template.replacen("%s", value, 1)
}

/// Roda e espera até o prazo; None se não rodou ou passou do prazo (o processo é morto). Devolve o código
/// de saída e a saída padrão.
pub fn run_timeout(command: &mut Command, timeout: Duration) -> Option<(i32, String)> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // A saída é lida numa linha à parte: um filho que escreve muito não trava esperando quem leia
    let reader = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.read_to_end(&mut text);
        String::from_utf8_lossy(&text).into_owned()
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().unwrap_or_default();
    Some((status.code().unwrap_or(1), out))
}

/// O `run` do sway_themes: o código de saída, 127 se o programa não existe ou passou de 10 s.
pub fn run(args: &[&str], dconf_profile: Option<&std::path::Path>) -> i32 {
    let Some((program, rest)) = args.split_first() else { return 127 };
    let mut command = Command::new(program);
    command.args(rest);
    if let Some(profile) = dconf_profile {
        command.env("DCONF_PROFILE", profile);
    }
    run_timeout(&mut command, Duration::from_secs(10)).map_or(127, |(code, _)| code)
}

/// O programa está no PATH (o shutil.which do Python).
pub fn which(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_sway_theme_only_beside_its_launcher() {
        let dir = std::env::temp_dir().join(format!("swaydesk-theme-command-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(command_beside(Some(&dir)), "swaydesk-theme"); // o /usr/bin do pacote
        std::fs::write(dir.join("sway-theme"), "#!/bin/sh\n").unwrap();
        assert_eq!(command_beside(Some(&dir)), "sway-theme"); // o ~/.local/bin dos dotfiles
        assert_eq!(command_beside(None), "swaydesk-theme");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
