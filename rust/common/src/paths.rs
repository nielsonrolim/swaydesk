//! Onde achar os scripts auxiliares e os outros componentes. Duas instalações convivem:
//! - os dotfiles (install.sh): os binários e os scripts lado a lado em ~/.local/bin, e os lançadores em
//!   Python, que escolhem entre a versão em Python e a em Rust (components no config);
//! - os pacotes: os binários em /usr/bin e os scripts em /usr/libexec/swaydesk (Debian) ou /usr/lib/swaydesk
//!   (Arch), sem os lançadores em Python.
use std::path::{Path, PathBuf};

use crate::config;

/// A pasta do executável em uso. O current_exe do Linux já vem resolvido (/proc/self/exe).
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

fn local_bin() -> PathBuf {
    config::home().join(".local/bin")
}

/// Os lugares de um script auxiliar, em ordem: a pasta dos scripts do pacote (bin/../libexec/swaydesk no
/// Debian, bin/../lib/swaydesk no Arch) e o ~/.local/bin dos dotfiles. Não a própria pasta do executável: num
/// pacote ela é o /usr/bin, e um sway-lock ou session-action de outro pacote passaria na frente.
pub fn helper_candidates(name: &str, exe_dir: Option<&Path>, home_bin: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = exe_dir {
        if let Some(prefix) = dir.parent() {
            found.push(prefix.join("libexec/swaydesk").join(name));
            found.push(prefix.join("lib/swaydesk").join(name));
        }
    }
    found.push(home_bin.join(name));
    found
}

/// Um script auxiliar (sway-caffeine, session-action…): o primeiro que existir, ou o de ~/.local/bin, como
/// antes, para a mensagem de erro dizer onde se esperava. Sem nenhum, avisa no stderr (o sway.log) uma vez
/// por nome, com os lugares procurados e o que instalar: sem o script, o botão só não fazia nada.
pub fn helper(name: &str) -> PathBuf {
    find_helper(name).unwrap_or_else(|| {
        if first_miss(name) {
            // O install.sh copia os binários para o ~/.local/bin; o exe_dir já vem resolvido, a HOME pode não vir
            let dotfiles = exe_dir().is_some_and(|dir| std::fs::canonicalize(local_bin()).is_ok_and(|bin| bin == dir));
            eprintln!("{}", missing_helper_message(name, &helper_search(name), dotfiles));
        }
        local_bin().join(name)
    })
}

/// O pacote que leva cada script auxiliar (desde a 0.1.7, o do componente que o chama: o SCRIPTS e o MODULES do
/// packaging/install-tree.sh, que o tests/test_packaging.py compara com esta lista). None para um nome que não é
/// de pacote nenhum.
fn helper_package(name: &str) -> Option<&'static str> {
    Some(match name {
        "session-action" | "waybar-notify" | "sway_i18n.py" | "desktop_config.py" | "sway-i18n.sh" => "swaydesk-common",
        "waybar-icon-lib" => "swaydesk-common",
        "sway-files" => "swaydesk-drawer",
        "sway-nightlight" | "waybar-brightness" => "swaydesk-brightness",
        "sway-osd" | "sway-brightness" => "swaydesk-osd",
        "sway-caffeine" | "sway-idle" | "sway-idle-suspend" | "waybar-caffeine" => "swaydesk-caffeine",
        "waybar-volume" => "swaydesk-audio",
        "waybar-media" => "swaydesk-media",
        "waybar-battery" => "swaydesk-battery",
        "sway-shot" | "sway-rec" | "sway-notify-open" => "swaydesk-capture",
        "sway-lock" | "sway-lock-watch" | "sway-gtklock" => "swaydesk-lock",
        "waybar_holidays.py" | "waybar-clock" => "swaydesk-calendar",
        _ => return None,
    })
}

/// O aviso de um script auxiliar que não existe em lugar nenhum: os lugares que o find_helper tentou e, nos
/// dotfiles (o binário no ~/.local/bin), o install.sh; nos pacotes, o pacote que o leva.
fn missing_helper_message(name: &str, tried: &[PathBuf], dotfiles: bool) -> String {
    let places: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
    let hint = match (dotfiles, helper_package(name)) {
        (true, _) => "run install.sh again".to_string(),
        (false, Some(package)) => format!("install the {package} package"),
        (false, None) => "reinstall the Swaydesk packages".to_string(),
    };
    format!("swaydesk: helper script {name} not found (tried {}); {hint}", places.join(", "))
}

/// Se é a primeira falta deste nome neste processo (o aviso sai uma vez, não a cada clique).
fn first_miss(name: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static MISSED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    MISSED.get_or_init(Default::default).lock().map(|mut seen| seen.insert(name.to_string())).unwrap_or(false)
}

/// Os lugares de um script, na ordem em que o find_helper procura. Na sessão do pacote, só os dele: o
/// ~/.local/bin é da outra sessão.
fn helper_search(name: &str) -> Vec<PathBuf> {
    let home_bin = local_bin();
    let mut candidates = helper_candidates(name, exe_dir().as_deref(), &home_bin);
    if package_session() {
        candidates.retain(|p| !p.starts_with(&home_bin));
    }
    candidates
}

/// O script auxiliar que existe, ou None.
pub fn find_helper(name: &str) -> Option<PathBuf> {
    helper_search(name).into_iter().find(|p| p.is_file())
}

/// Um script auxiliar que nunca pode vir do PATH nem da HOME de outro lugar, porque age de verdade (o
/// session-action reinicia e desliga a máquina): a pasta dos scripts do pacote ou a própria pasta do
/// executável (o ~/.local/bin dos dotfiles; nos testes, a pasta do teste, com um falso ao lado). None se
/// nenhum existir.
pub fn helper_beside_exe(name: &str) -> Option<PathBuf> {
    let dir = exe_dir()?;
    helper_candidates(name, Some(&dir), &dir).into_iter().find(|p| p.is_file())
}

/// O aviso de um helper_beside_exe que voltou None: os lugares que ele tentou e o que instalar (o pacote dono,
/// ou o install.sh nos dotfiles), o mesmo texto do helper.
pub fn missing_beside_exe_message(name: &str) -> String {
    let dir = exe_dir();
    let tried = dir.as_deref().map(|d| helper_candidates(name, Some(d), d)).unwrap_or_default();
    let dotfiles = dir.is_some_and(|dir| std::fs::canonicalize(local_bin()).is_ok_and(|bin| bin == dir));
    missing_helper_message(name, &tried, dotfiles)
}

/// Se este processo está na sessão do pacote swaydesk-desktop (o sway-session --package exporta
/// SWAYDESK_SESSION=package). Lá, nada vem do ~/.local/bin, mesmo que os dotfiles estejam instalados.
pub fn package_session() -> bool {
    std::env::var_os("SWAYDESK_SESSION").is_some_and(|v| v == "package")
}

/// Outro componente: o lançador em Python de ~/.local/bin (dotfiles: ele respeita o components do config)
/// ou, sem ele, o binário em Rust ao lado deste ou no PATH (pacotes). Sem nenhum, o lançador de
/// ~/.local/bin, como antes. Na sessão do pacote, o lançador dos dotfiles não conta.
pub fn component(launcher: &str, binary: &str) -> PathBuf {
    let home_launcher = local_bin().join(launcher);
    if !package_session() && home_launcher.is_file() {
        return home_launcher;
    }
    if let Some(beside) = exe_dir().map(|d| d.join(binary)).filter(|p| p.is_file()) {
        return beside;
    }
    std::env::var_os("PATH")
        .and_then(|path| std::env::split_paths(&path).map(|d| d.join(binary)).find(|p| p.is_file()))
        .unwrap_or(home_launcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_helper_is_reported_once_with_where_it_looked() {
        let tried = [PathBuf::from("/usr/local/libexec/swaydesk/sway-files"), PathBuf::from("/usr/local/lib/swaydesk/sway-files")];
        let package = missing_helper_message("sway-files", &tried, false);
        assert_eq!(
            package,
            "swaydesk: helper script sway-files not found (tried /usr/local/libexec/swaydesk/sway-files, \
             /usr/local/lib/swaydesk/sway-files); install the swaydesk-drawer package"
        );
        assert!(missing_helper_message("sway-lock", &tried, false).ends_with("; install the swaydesk-lock package"));
        assert!(missing_helper_message("no-such-helper", &tried, false).ends_with("; reinstall the Swaydesk packages"));
        let dotfiles = missing_helper_message("sway-files", &[PathBuf::from("/home/u/.local/bin/sway-files")], true);
        assert!(dotfiles.ends_with("(tried /home/u/.local/bin/sway-files); run install.sh again"), "{dotfiles}");
        // Um nome só deste teste, para não depender da ordem dos outros
        assert!(first_miss("a-helper-only-this-test-misses"));
        assert!(!first_miss("a-helper-only-this-test-misses"));
        assert!(first_miss("another-helper-only-this-test-misses"));
    }

    #[test]
    fn helpers_beside_then_package_then_home() {
        let home = Path::new("/home/u/.local/bin");
        assert_eq!(
            helper_candidates("sway-caffeine", Some(Path::new("/usr/bin")), home),
            [
                PathBuf::from("/usr/libexec/swaydesk/sway-caffeine"),
                PathBuf::from("/usr/lib/swaydesk/sway-caffeine"),
                PathBuf::from("/home/u/.local/bin/sway-caffeine")
            ]
        );
        // Nos dotfiles, o binário está no próprio ~/.local/bin: ~/.local/libexec e ~/.local/lib não têm os
        // scripts, e o último é o certo
        assert_eq!(helper_candidates("x", Some(home), home).last(), Some(&PathBuf::from("/home/u/.local/bin/x")));
        assert_eq!(helper_candidates("x", None, home), [PathBuf::from("/home/u/.local/bin/x")]);
        // O helper_beside_exe: a pasta do pacote primeiro (um session-action de outro pacote em /usr/bin não
        // passa na frente) e, por último, ao lado do executável, nunca o ~/.local/bin
        assert_eq!(
            helper_candidates("session-action", Some(Path::new("/usr/bin")), Path::new("/usr/bin")),
            [
                PathBuf::from("/usr/libexec/swaydesk/session-action"),
                PathBuf::from("/usr/lib/swaydesk/session-action"),
                PathBuf::from("/usr/bin/session-action")
            ]
        );
    }
}
