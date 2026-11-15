//! O primeiro login na sessão do pacote swaydesk-desktop: os arquivos de cada usuário (o `home-copy/` do
//! repositório, que o pacote instala como `skel`) vão para o `~`, com as mesmas regras do `install.sh`:
//! - só o que falta: um arquivo que já existe (mesmo um link quebrado) é do usuário e fica;
//! - o `50-local.conf` ganha o teclado do sistema (`/etc/default/keyboard`), como o `keyboard_block`; sem esse
//!   arquivo (Arch), o do systemd: o layout X11 do localectl, senão o mapa do console traduzido pelo
//!   kbd-model-map;
//! - os fixados da dock trocam o Nemo do modelo pelo gerenciador de arquivos da config.
use std::path::{Path, PathBuf};

use swaydesk_common::config;
use swaydesk_common::keyboard as kbd;

use crate::desktop;

const LOCAL_CONF: &str = ".config/sway/config.d/50-local.conf";
const PINNED: &str = ".local/share/sway-dock/pinned";
const CONFIG: &str = ".config/sway-desktop/config.toml";

/// O skel do pacote: SWAYDESK_SKEL, senão o swaydesk-desktop/skel ao lado do share/sway-desktop da instalação
/// (/usr/share/swaydesk-desktop/skel, para o binário em /usr/bin).
pub fn skel_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SWAYDESK_SKEL").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let share = desktop::share_dir();
    share.parent().map(|p| p.join("swaydesk-desktop/skel")).unwrap_or_else(|| share.join("../swaydesk-desktop/skel"))
}

/// Os arquivos do skel, relativos a ele e em ordem (a config.toml vem antes dos fixados da dock). Só arquivos
/// comuns: um link no skel (o home-copy/ não tem nenhum) ficaria de fora.
fn skel_files(skel: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![skel.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => {
                    if let Ok(rel) = path.strip_prefix(skel) {
                        files.push(rel.to_path_buf());
                    }
                }
                _ => {}
            }
        }
    }
    files.sort();
    files
}

/// Troca a linha "nemo" pelo .desktop do gerenciador da config, se for um nome seguro.
fn pinned_with(text: &str, file_manager: &str) -> String {
    if file_manager.is_empty() || !file_manager.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) {
        return text.to_string();
    }
    text.lines().map(|line| if line == "nemo" { file_manager } else { line }).map(|line| format!("{line}\n")).collect()
}

/// A config.toml do skel com um tema GTK que exista: o padrão é o Mint-Y-Dark-Teal, que só o Mint tem. No
/// Debian, no Ubuntu e no Arch, fica o tema de reserva (o Adwaita-dark), em vez de o app Tema avisar que o
/// tema da config não está instalado. No Mint, nada muda.
fn with_installed_theme(text: &str, installed: &dyn Fn(&str) -> bool) -> String {
    text.lines()
        .map(|line| match line.strip_prefix("gtk_theme = \"").and_then(|rest| rest.strip_suffix('"')) {
            Some(theme) if !installed(theme) => format!("gtk_theme = \"{}\"\n", desktop::FALLBACK_THEME),
            _ => format!("{line}\n"),
        })
        .collect()
}

/// Copia do skel para o home o que falta. Devolve os caminhos copiados, relativos ao home.
pub fn setup(skel: &Path, home: &Path, keyboard: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    for rel in skel_files(skel) {
        let target = home.join(&rel);
        // symlink_metadata: um link quebrado também é do usuário
        if target.symlink_metadata().is_ok() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(skel.join(&rel), &target)?;
        if rel == Path::new(CONFIG) {
            let text = std::fs::read_to_string(&target)?;
            std::fs::write(&target, with_installed_theme(&text, &desktop::theme_installed))?;
        } else if rel == Path::new(LOCAL_CONF) {
            let block = kbd::system_keyboard(keyboard)
                .map(|(source, text)| kbd::keyboard_block_text(&source, &text))
                .unwrap_or_default();
            if !block.is_empty() {
                let text = std::fs::read_to_string(&target)?;
                std::fs::write(&target, text + &block)?;
            }
        } else if rel == Path::new(PINNED) {
            // A config.toml, se veio do skel, já foi copiada: vem antes na ordem. O config::load lê a do HOME
            // do ambiente, que no CLI é este home
            let manager = desktop::get(&config::load(), "file_manager_desktop").unwrap_or_default();
            let text = std::fs::read_to_string(&target)?;
            std::fs::write(&target, pinned_with(&text, &manager))?;
        }
        copied.push(rel);
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pinned_takes_the_file_manager() {
        assert_eq!(pinned_with("firefox\nnemo\nkitty\n", "thunar"), "firefox\nthunar\nkitty\n");
        assert_eq!(pinned_with("nemo\n", "a b"), "nemo\n");
        assert_eq!(pinned_with("nemo\n", ""), "nemo\n");
    }

    #[test]
    fn a_missing_gtk_theme_becomes_the_fallback() {
        let text = "# o tema\ngtk_theme = \"Mint-Y-Dark-Teal\"\nicon_theme = \"Papirus-Dark\"\n";
        assert_eq!(with_installed_theme(text, &|_| true), text);
        assert_eq!(with_installed_theme(text, &|_| false),
                   format!("# o tema\ngtk_theme = \"{}\"\nicon_theme = \"Papirus-Dark\"\n", desktop::FALLBACK_THEME));
    }

    #[test]
    fn copies_only_what_is_missing() {
        let dir = tempdir("copy");
        let (skel, home) = (dir.join("skel"), dir.join("home"));
        for (rel, text) in [(".config/a/one", "1\n"), (".config/a/two", "2\n"), (".local/b/three", "3\n")] {
            std::fs::create_dir_all(skel.join(rel).parent().unwrap()).unwrap();
            std::fs::write(skel.join(rel), text).unwrap();
        }
        std::fs::create_dir_all(home.join(".config/a")).unwrap();
        std::fs::write(home.join(".config/a/one"), "meu\n").unwrap();
        std::os::unix::fs::symlink(dir.join("nao-existe"), home.join(".config/a/two")).unwrap();
        let copied = setup(&skel, &home, &dir.join("sem-teclado")).unwrap();
        assert_eq!(copied, vec![PathBuf::from(".local/b/three")]);
        assert_eq!(std::fs::read_to_string(home.join(".config/a/one")).unwrap(), "meu\n");
        assert!(home.join(".config/a/two").symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(home.join(".local/b/three")).unwrap(), "3\n");
        // De novo: nada a copiar
        assert!(setup(&skel, &home, &dir.join("sem-teclado")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
