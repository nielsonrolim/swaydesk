//! O `--help` e o `--version` de todo programa do Swaydesk (0.1.8): respondidos na primeira linha do `main`,
//! antes do GTK, do D-Bus e de qualquer socket, e assim sem display. Rodado à mão num Sway puro, um `--help`
//! abria a janela ou esperava pela tela. O `tests/test_help_flags.py` (EveryBinaryTests) roda todo binário
//! do `packaging/components.list` sem display e sem barramento.
//!
//! Uso, no começo do `main`: `swaydesk_common::early_args!(USAGE);` (um `&str` ou uma `String`, só montada
//! se o `--help` vier). Só um argumento sozinho conta: `--confirm --help` segue para o programa.
use std::io::Write;

/// A versão dos pacotes (o build.rs: a chave release de uma correção X.Y.Z.W, ou a [workspace.package] version)
pub const VERSION: &str = env!("SWAYDESK_VERSION");

/// O que o argumento único pede
#[derive(Debug, PartialEq, Eq)]
pub enum Early {
    Help,
    Version,
}

pub fn early(args: &[String]) -> Option<Early> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => Some(Early::Help),
        [flag] if flag == "--version" => Some(Early::Version),
        _ => None,
    }
}

/// O texto da resposta, ou nada se os argumentos não pedem uma: `<binário> <versão>` no `--version`
pub fn reply(args: &[String], program: &str, version: &str, usage: impl FnOnce() -> String) -> Option<String> {
    Some(match early(args)? {
        Early::Help => usage(),
        Early::Version => format!("{program} {version}"),
    })
}

/// Responde e sai (0; 1 se a saída falhou, um pipe fechado) quando os argumentos pedem; senão, volta
pub fn answer(program: &str, version: &str, usage: impl FnOnce() -> String) {
    // Sem o args(), que entra em pânico num argumento que não é UTF-8
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let Some(text) = reply(&args, program, version, usage) else {
        return;
    };
    let mut out = std::io::stdout().lock();
    let written = writeln!(out, "{}", text.trim_end()).and_then(|()| out.flush());
    std::process::exit(if written.is_ok() { 0 } else { 1 });
}

/// `--help` e `--version` com o nome do binário de quem chama e a versão dos pacotes
#[macro_export]
macro_rules! early_args {
    ($usage:expr) => {
        $crate::cli::answer(env!("CARGO_BIN_NAME"), $crate::cli::VERSION, || ::std::string::String::from($usage))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn only_a_lone_flag_counts() {
        assert_eq!(early(&args(&["--help"])), Some(Early::Help));
        assert_eq!(early(&args(&["-h"])), Some(Early::Help));
        assert_eq!(early(&args(&["--version"])), Some(Early::Version));
        assert_eq!(early(&args(&[])), None);
        assert_eq!(early(&args(&["--check"])), None);
        assert_eq!(early(&args(&["--confirm", "--help"])), None);
        assert_eq!(early(&args(&["--version", "x"])), None);
        assert_eq!(early(&args(&["-V"])), None);
    }

    #[test]
    fn the_reply_names_the_binary_and_its_version() {
        let usage = || "usage: swaydesk-x".to_string();
        assert_eq!(reply(&args(&["--version"]), "swaydesk-x", "0.1.8", usage).as_deref(), Some("swaydesk-x 0.1.8"));
        assert_eq!(reply(&args(&["--help"]), "swaydesk-x", "0.1.8", usage).as_deref(), Some("usage: swaydesk-x"));
        assert_eq!(reply(&args(&["daemon"]), "swaydesk-x", "0.1.8", || unreachable!()), None);
    }
}
