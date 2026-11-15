//! O tema do cursor da sessão: o Bibata Modern Classic, que o Debian, o Ubuntu e o Mint empacotam; onde ele
//! não existe (o Arch só o tem no AUR), o Capitaine, o mais parecido nos repositórios oficiais; sem nenhum,
//! None e o padrão do sistema. O mesmo do `CURSOR_THEMES` do `sway_themes.py`.
use std::path::{Path, PathBuf};

use crate::config;

/// Na ordem de preferência.
pub const CURSOR_THEMES: [&str; 2] = ["Bibata-Modern-Classic", "capitaine-cursors"];
pub const CURSOR_SIZE: u32 = 24;

/// Onde o Xcursor e o GTK procuram os temas, do que vence para o que perde. SWAY_SYSTEM_ICONS troca o
/// /usr/share/icons nos testes.
pub fn icon_dirs() -> Vec<PathBuf> {
    let system = std::env::var("SWAY_SYSTEM_ICONS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/share/icons".into());
    vec![config::home().join(".icons"), config::data_home().join("icons"), PathBuf::from(system)]
}

/// O primeiro de CURSOR_THEMES com cursores numa das pastas.
pub fn cursor_theme_in(dirs: &[PathBuf]) -> Option<&'static str> {
    CURSOR_THEMES.into_iter().find(|name| dirs.iter().any(|dir| has_cursors(&dir.join(name))))
}

pub fn cursor_theme() -> Option<&'static str> {
    cursor_theme_in(&icon_dirs())
}

/// Um tema de ícones sem a pasta cursors/ não é um tema de cursor.
fn has_cursors(theme: &Path) -> bool {
    theme.join("cursors").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-cursor-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn with_cursors(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir.join(name).join("cursors")).unwrap();
    }

    #[test]
    fn prefers_bibata_then_capitaine_then_none() {
        let tmp = tmp("order");
        let (user, system) = (tmp.join("user"), tmp.join("system"));
        let dirs = [user.clone(), system.clone()];
        assert_eq!(cursor_theme_in(&dirs), None);
        with_cursors(&system, "capitaine-cursors");
        assert_eq!(cursor_theme_in(&dirs), Some("capitaine-cursors"));
        with_cursors(&user, "Bibata-Modern-Classic");
        assert_eq!(cursor_theme_in(&dirs), Some("Bibata-Modern-Classic"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_icon_theme_without_cursors_does_not_count() {
        let tmp = tmp("icons");
        std::fs::create_dir_all(tmp.join("Bibata-Modern-Classic")).unwrap();
        std::fs::write(tmp.join("Bibata-Modern-Classic/index.theme"), "[Icon Theme]\n").unwrap();
        assert_eq!(cursor_theme_in(&[tmp.clone()]), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
