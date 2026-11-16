//! Os perfis de atalhos, sem GTK: quais a Ajuda oferece e onde o usuário muda um atalho. A tabela
//! (SHORTCUT_PROFILES), o arquivo de cada perfil (shortcut_keys_file) e o perfil em uso (Config.shortcuts) são
//! do swaydesk_common::config.
use std::path::PathBuf;

use crate::{main_config_path, Env};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

/// O arquivo dos atalhos do usuário: o config.d/50-local.conf do ~/.config/sway (ou do XDG_CONFIG_HOME). O
/// primeiro login o cria, nada o sobrescreve, e o Sway o lê depois do perfil: um bind ali troca o do perfil.
/// Na sessão do pacote também é este, o do usuário.
pub fn local_shortcuts_file(env: &Env) -> PathBuf {
    main_config_path(env, false, None).with_file_name("config.d").join("50-local.conf")
}

/// Os perfis que o seletor mostra: os da tabela cujo arquivo existe, na ordem da tabela. Um arquivo que falta
/// só tira o perfil da lista.
pub fn available(table: &[(&str, &str)], exists: impl Fn(&str) -> bool) -> Vec<Profile> {
    table
        .iter()
        .filter(|(id, _)| exists(id))
        .map(|(id, name)| Profile { id: (*id).to_string(), name: (*name).to_string() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use swaydesk_common::config::SHORTCUT_PROFILES;

    #[test]
    fn only_profiles_with_a_file_in_table_order() {
        let got = available(SHORTCUT_PROFILES, |id| id != "niri");
        let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["swaydesk", "i3", "hyprland", "dwm"]);
        assert_eq!(got[2].name, "Hyprland");
        assert!(available(SHORTCUT_PROFILES, |_| false).is_empty());
    }

    #[test]
    fn the_local_shortcuts_file() {
        let env = Env { home: "/home/u".into(), vars: Default::default() };
        assert_eq!(local_shortcuts_file(&env), Path::new("/home/u/.config/sway/config.d/50-local.conf"));
        let vars = [("XDG_CONFIG_HOME".to_string(), "/x/cfg".to_string())].into_iter().collect();
        let env = Env { home: "/home/u".into(), vars };
        assert_eq!(local_shortcuts_file(&env), Path::new("/x/cfg/sway/config.d/50-local.conf"));
    }
}
