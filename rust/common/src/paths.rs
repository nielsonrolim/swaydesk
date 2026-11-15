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
/// antes, para a mensagem de erro dizer onde se esperava.
pub fn helper(name: &str) -> PathBuf {
    find_helper(name).unwrap_or_else(|| local_bin().join(name))
}

/// O script auxiliar que existe, ou None. Na sessão do pacote, só os dele: o ~/.local/bin é da outra sessão.
pub fn find_helper(name: &str) -> Option<PathBuf> {
    let home_bin = local_bin();
    let mut candidates = helper_candidates(name, exe_dir().as_deref(), &home_bin);
    if package_session() {
        candidates.retain(|p| !p.starts_with(&home_bin));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Um script auxiliar que nunca pode vir do PATH nem da HOME de outro lugar, porque age de verdade (o
/// session-action reinicia e desliga a máquina): a pasta dos scripts do pacote ou a própria pasta do
/// executável (o ~/.local/bin dos dotfiles; nos testes, a pasta do teste, com um falso ao lado). None se
/// nenhum existir.
pub fn helper_beside_exe(name: &str) -> Option<PathBuf> {
    let dir = exe_dir()?;
    helper_candidates(name, Some(&dir), &dir).into_iter().find(|p| p.is_file())
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
