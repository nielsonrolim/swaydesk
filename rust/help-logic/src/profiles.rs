//! Os perfis de atalhos, sem GTK: quais a Ajuda oferece e onde o usuário muda um atalho. A tabela
//! (SHORTCUT_PROFILES), o arquivo de cada perfil (shortcut_keys_file) e o perfil em uso (Config.shortcuts) são
//! do swaydesk_common::config.
use std::path::{Path, PathBuf};

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

/// O terminal do Swaydesk (o $term do config do Sway): uma dependência do swaydesk-desktop.
pub const TERMINAL: &str = "kitty";

/// O script do editor no terminal: o $VISUAL, o $EDITOR ou o primeiro de nano, vim e vi que existir (o Arch não
/// traz nenhum no base). Sem editor nenhum, mostra a mensagem e espera o Enter. O arquivo e a mensagem vão como
/// argumentos ($1 e $2), nunca no texto do script. O $e fica sem aspas de propósito: um $EDITOR como "code -w".
const EDITOR_SCRIPT: &str = r#"e=${VISUAL:-${EDITOR:-}}
if [ -z "$e" ]; then
  for c in nano vim vi; do
    if command -v "$c" >/dev/null 2>&1; then e=$c; break; fi
  done
fi
if [ -z "$e" ]; then printf '%s\n' "$2"; read -r _; exit 1; fi
exec $e "$1""#;

/// Abrir um arquivo de texto num editor do terminal, quando não há app gráfico para text/plain (decisão do
/// usuário): o terminal do Swaydesk com o editor do usuário. `message` aparece se não houver editor nenhum.
pub fn terminal_editor_command(file: &Path, message: &str) -> Vec<String> {
    vec![
        TERMINAL.to_string(),
        "-e".to_string(),
        "sh".to_string(),
        "-c".to_string(),
        EDITOR_SCRIPT.to_string(),
        "sh".to_string(),
        file.display().to_string(),
        message.to_string(),
    ]
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

    /// Roda o script do editor sem o terminal (do "sh" em diante), com um PATH só de editores falsos que
    /// gravam o próprio nome e o arquivo. Devolve (o que gravaram, a saída, o código).
    fn run_editor(editors: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
        let dir = std::env::temp_dir().join(format!("help-editor-{}-{}", std::process::id(), editors.join("-")));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = dir.join("log");
        for name in editors {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\necho \"{name} $*\" >> '{}'\n", log.display())).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let args = terminal_editor_command(Path::new("/x/a b.conf"), "no editor");
        let mut cmd = std::process::Command::new("/bin/sh");
        // Só os editores falsos no PATH: os reais do sistema não contam
        cmd.args(&args[3..]).env_clear().env("PATH", bin.display().to_string());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.stdin(std::process::Stdio::null()).output().unwrap();
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        (logged.trim().to_string(), String::from_utf8_lossy(&out.stdout).trim().to_string(), out.status.code().unwrap_or(-1))
    }

    #[test]
    fn the_terminal_editor_order() {
        assert_eq!(terminal_editor_command(Path::new("/f"), "m")[..3], ["kitty", "-e", "sh"]);
        // O VISUAL vence, depois o EDITOR (com argumentos), depois nano, vim, vi; o arquivo com espaço chega inteiro
        assert_eq!(run_editor(&["nano", "myed"], &[("VISUAL", "myed"), ("EDITOR", "nano")]).0, "myed /x/a b.conf");
        assert_eq!(run_editor(&["nano", "myed"], &[("EDITOR", "myed -w")]).0, "myed -w /x/a b.conf");
        assert_eq!(run_editor(&["vim", "nano"], &[]).0, "nano /x/a b.conf");
        assert_eq!(run_editor(&["vi", "vim"], &[]).0, "vim /x/a b.conf");
        assert_eq!(run_editor(&["vi"], &[]).0, "vi /x/a b.conf");
        // Sem editor: a mensagem, e sai com 1
        let (logged, out, code) = run_editor(&[], &[]);
        assert_eq!((logged.as_str(), out.as_str(), code), ("", "no editor", 1));
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
