//! A prévia pendente: o layout de antes do Aplicar, enquanto o "Manter ou desfazer" está aberto. Fica no
//! XDG_RUNTIME_DIR (só da sessão): o app o grava no Aplicar e o apaga no Manter ou depois de desfazer. Se o app
//! some com a pergunta aberta sem desfazer (um crash, um SIGKILL), a próxima vez que ele abre desfaz por este
//! arquivo. Os comandos se refazem das configurações lidas (com os mesmos limites dos perfis), não do texto.
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::{safe_name, OutputConfig};
use crate::profiles::{config_from_json, config_to_json};

/// O arquivo, só no XDG_RUNTIME_DIR da sessão (0700, do usuário). Sem ele, nenhum: o /tmp de reserva do
/// resident é de todos, e um link plantado ali faria o app gravar por cima de outro arquivo
pub fn path() -> Option<PathBuf> {
    path_in(std::env::var_os("XDG_RUNTIME_DIR"))
}

fn path_in(runtime: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(runtime?);
    dir.is_absolute().then(|| dir.join("sway-displays-pending.json"))
}

/// Vale desfazer pelo arquivo? Só com as mesmas saídas de quando ele foi gravado: depois de um crash, o app pode
/// abrir muito mais tarde, com outros monitores
pub fn same_outputs(previous: &[(String, OutputConfig)], current: &[String]) -> bool {
    let mut saved: Vec<&str> = previous.iter().map(|(name, _)| name.as_str()).collect();
    let mut now: Vec<&str> = current.iter().map(String::as_str).collect();
    saved.sort_unstable();
    now.sort_unstable();
    saved == now
}

/// O arquivo: as saídas de antes, com o nome e a configuração de cada uma
pub fn to_json(previous: &[(String, OutputConfig)]) -> String {
    let outputs: Vec<Value> = previous.iter().map(|(name, config)| json!([name, config_to_json(config)])).collect();
    json!({ "outputs": outputs }).to_string()
}

/// O layout de antes; None num arquivo estragado ou sem nenhuma saída válida
pub fn from_json(text: &str) -> Option<Vec<(String, OutputConfig)>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let previous = value
        .get("outputs")?
        .as_array()?
        .iter()
        .map(|item| {
            let name = item.get(0)?.as_str().filter(|n| safe_name(n))?;
            Some((name.to_string(), config_from_json(item.get(1)?)?))
        })
        .collect::<Option<Vec<_>>>()?;
    (!previous.is_empty()).then_some(previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Transform;
    use crate::outputs::Mode;

    fn config(x: i32, scale: f64, enabled: bool) -> OutputConfig {
        OutputConfig {
            enabled,
            mode: Some(Mode { width: 1920, height: 1080, refresh: 60000 }),
            x,
            y: 0,
            scale,
            transform: Transform::Normal,
            adaptive_sync: false,
        }
    }

    #[test]
    fn the_file_lives_only_in_the_session_runtime_dir() {
        assert_eq!(path_in(Some("/run/user/1000".into())), Some(PathBuf::from("/run/user/1000/sway-displays-pending.json")));
        assert_eq!(path_in(None), None);
        assert_eq!(path_in(Some("".into())), None);
        // Um caminho relativo seria o da pasta de onde o app foi aberto
        assert_eq!(path_in(Some("run/user".into())), None);
    }

    #[test]
    fn only_the_same_outputs_are_restored() {
        let previous = vec![("DP-1".to_string(), config(0, 1.0, true)), ("HDMI-A-1".to_string(), config(1920, 1.5, false))];
        assert!(same_outputs(&previous, &["HDMI-A-1".to_string(), "DP-1".to_string()]));
        assert!(!same_outputs(&previous, &["DP-1".to_string()]));
        assert!(!same_outputs(&previous, &["DP-1".to_string(), "HDMI-A-1".to_string(), "DP-2".to_string()]));
        assert!(!same_outputs(&previous, &["DP-2".to_string(), "HDMI-A-1".to_string()]));
    }

    #[test]
    fn the_previous_layout_comes_back_from_the_file() {
        let previous = vec![("DP-1".to_string(), config(0, 1.0, true)), ("HDMI-A-1".to_string(), config(1920, 1.5, false))];
        assert_eq!(from_json(&to_json(&previous)), Some(previous));
    }

    #[test]
    fn a_broken_file_restores_nothing() {
        for text in ["", "{", "[]", "{\"outputs\": []}", "{\"outputs\": [[\"DP-1\", {\"enabled\": true}]]}"] {
            assert_eq!(from_json(text), None, "{text}");
        }
    }

    #[test]
    fn a_name_that_is_not_an_output_restores_nothing() {
        // O arquivo vira comandos do Sway: um nome com ; ou espaço não passa (o mesmo safe_name dos comandos)
        let text = to_json(&[("DP-1; exec foot".to_string(), config(0, 1.0, true))]);
        assert_eq!(from_json(&text), None);
    }

    #[test]
    fn a_value_out_of_range_invalidates_the_whole_layout() {
        // Como num perfil: desfazer pela metade seria pior que não desfazer
        let good = to_json(&[("DP-1".to_string(), config(0, 1.0, true)), ("DP-2".to_string(), config(0, 1.0, true))]);
        assert!(from_json(&good).is_some(), "{good}");
        let text = good.replacen("1.0", "9.0", 1);
        assert_eq!(from_json(&text), None, "{text}");
    }
}
