//! Os atalhos da janela de ajuda (swaydesk-help), lidos do config do Sway em uso, sem GTK.
//!
//! O config principal vem do próprio Sway (o GET_CONFIG do IPC): é o que está carregado, o
//! ~/.config/sway/config dos dotfiles ou o do pacote. Os `include` são lidos do disco, como o Sway faz, e
//! um atalho do config.d/50-local.conf aparece sozinho. A descrição de cada atalho é um comentário logo
//! acima dele:
//!
//! ```text
//! ### Windows                   <- abre uma seção, até o próximo ###
//! ## Close the window           <- descreve o bind seguinte e os colados a ele
//! bindsym $mod+q kill
//! ```
//!
//! Os textos do config do projeto estão em inglês e são traduzidos pelo po/ (o po/update os extrai); os do
//! usuário aparecem como foram escritos. Um bind sem `##` aparece com o comando.
pub mod glob;
pub mod ipc;
pub mod keys;
pub mod parse;

use std::path::{Path, PathBuf};

pub use keys::{chord, chords, find_shortcut, short_command, Chord, Label};
pub use parse::{parse, Bind, Entry, Env, Kind};

/// O config principal: o do pacote na sessão do pacote (bin/../share/swaydesk-desktop/sway/config, como o
/// sway-session o abre) e, fora dela, o do usuário. Serve de pasta para os `include` relativos e de reserva
/// quando o Sway não responde.
pub fn main_config_path(env: &Env, package: bool, exe: Option<&Path>) -> PathBuf {
    if package {
        if let Some(prefix) = exe.and_then(Path::parent).and_then(Path::parent) {
            return prefix.join("share/swaydesk-desktop/sway/config");
        }
    }
    let config_home = env
        .var("XDG_CONFIG_HOME")
        .filter(|dir| Path::new(dir).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home.join(".config"));
    config_home.join("sway/config")
}

/// Os atalhos do Sway em uso: o texto do IPC, ou o arquivo se o Sway não responder (fora dele, nos testes).
pub fn load(env: &Env) -> Vec<Entry> {
    let package = env.var("SWAYDESK_SESSION").as_deref() == Some("package");
    let exe = std::env::current_exe().ok();
    let main = main_config_path(env, package, exe.as_deref());
    let from_sway = env.var("SWAYSOCK").and_then(|sock| ipc::get_config(Path::new(&sock)).ok());
    match from_sway {
        Some(text) => parse(&text, &main, env),
        None => match parse::read_limited(&main) {
            Some(text) => parse(&text, &main, env),
            None => Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests;
