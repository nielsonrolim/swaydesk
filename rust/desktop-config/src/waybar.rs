//! O config da Waybar sem os módulos desligados no [enabled.bar]: o `waybar_config` do desktop_config.py.
//!
//! O modelo é o config.jsonc do pacote ou o dos dotfiles. O resultado vai para
//! ~/.config/swaydesk-desktop/waybar/config.jsonc, que o sway-waybar usa; se a geração falhar, ele sobe a
//! Waybar com o modelo (a barra nunca some por causa do [enabled]).
use std::path::{Path, PathBuf};

use serde_json::Value;
use swaydesk_common::config::{Config, BAR_MODULES};
use swaydesk_theme::desktop as dc;

/// Na pasta de config, como o style.css do pacote.
pub const GENERATED: &str = "swaydesk-desktop/waybar/config.jsonc";
const HEADER: &str = "// Gerado pelo sway-desktop-config a partir do modelo, sem os módulos desligados no [enabled.bar]\n";

/// O JSONC sem os comentários (// e /* */ fora das strings).
pub fn strip_jsonc(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else if b[i..].starts_with(b"//") {
            i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
        } else if b[i..].starts_with(b"/*") {
            i = b[i + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |p| i + 2 + p + 2);
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    // Só se corta nas fronteiras de ASCII (aspas, barras, quebra de linha): o UTF-8 continua inteiro
    String::from_utf8(out).unwrap_or_default()
}

/// O módulo das atualizações do sistema (o sway-updates), só onde há o checkupdates do pacman-contrib: o Arch. No
/// Debian e no Ubuntu a loja avisa do apt, e o config fica como era.
pub const UPDATES_MODULE: &str = "custom/updates";

/// O config filtrado, em JSON. Err se o modelo não for um JSONC válido. `checkupdates`: se o programa existe.
pub fn filtered(template: &str, config: &Config, checkupdates: bool) -> Result<String, String> {
    let mut data: Value = serde_json::from_str(&strip_jsonc(template)).map_err(|e| e.to_string())?;
    let mut off: Vec<&str> = BAR_MODULES
        .iter()
        .filter(|(key, _)| !config.enabled(&format!("bar.{key}")))
        .map(|(_, module)| *module)
        .collect();
    // Com o update_notifier = false, o ícone some junto com a verificação: sem ela, o cache ficaria velho
    if !checkupdates || !config.enabled("update_notifier") {
        off.push(UPDATES_MODULE);
    }
    let bars: Vec<&mut Value> = match &mut data {
        Value::Array(list) => list.iter_mut().collect(),
        other => vec![other],
    };
    for bar in bars {
        let Value::Object(bar) = bar else { return Err("a bar is not an object".into()) };
        for side in ["modules-left", "modules-center", "modules-right"] {
            if let Some(Value::Array(modules)) = bar.get_mut(side) {
                modules.retain(|m| m.as_str().is_none_or(|m| !off.contains(&m)));
            }
        }
    }
    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    Ok(format!("{HEADER}{json}\n"))
}

/// O modelo: o do pacote (bin/../share/swaydesk-desktop/waybar) na sessão do pacote; fora dela, o dos
/// dotfiles (~/.config/waybar/config.jsonc), ou o do pacote se não houver o dos dotfiles.
pub fn default_template(package: bool, exe: Option<&Path>, config_home: &Path) -> PathBuf {
    let packaged = exe
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|prefix| prefix.join("share/swaydesk-desktop/waybar/config.jsonc"));
    let dotfiles = config_home.join("waybar/config.jsonc");
    match packaged {
        Some(p) if package || (!dotfiles.exists() && p.exists()) => p,
        _ => dotfiles,
    }
}

/// Grava o filtrado e devolve o caminho.
pub fn write(config: &Config, template: Option<&Path>) -> Result<PathBuf, String> {
    let config_home = dc::config_home();
    let template = match template {
        Some(t) => t.to_path_buf(),
        None => {
            let package = std::env::var_os("SWAYDESK_SESSION").is_some_and(|v| v == "package");
            default_template(package, std::env::current_exe().ok().as_deref(), &config_home)
        }
    };
    let text = std::fs::read_to_string(&template).map_err(|e| format!("{}: {e}", template.display()))?;
    let out = config_home.join(GENERATED);
    dc::write_file(&out, &filtered(&text, config, swaydesk_theme::which("checkupdates"))?).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swaydesk_common::config::parse_config;

    const TEMPLATE: &str = r#"{
  // comentário com "aspas"
  "modules-left": ["sway/workspaces"], /* em bloco */
  "modules-center": ["custom/weather", "custom/clock"],
  "modules-right": ["tray", "custom/rec", "custom/language", "custom/power"],
  "custom/clock": { "format": "{:%H:%M} // não é comentário", "exec": "a\"b" }
}"#;

    #[test]
    fn drops_only_the_disabled_modules() {
        let (config, warnings) = parse_config("[enabled.bar]\nweather = false\nrecording = false\nclock = true\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        let out = filtered(TEMPLATE, &config, true).unwrap();
        assert!(out.starts_with("// Gerado"));
        let data: Value = serde_json::from_str(&strip_jsonc(&out)).unwrap();
        assert_eq!(data["modules-center"], serde_json::json!(["custom/clock"]));
        assert_eq!(data["modules-right"], serde_json::json!(["tray", "custom/language", "custom/power"]));
        assert_eq!(data["custom/clock"]["format"], "{:%H:%M} // não é comentário");
        assert_eq!(data["custom/clock"]["exec"], "a\"b");
    }

    #[test]
    fn nothing_disabled_keeps_every_module_and_bad_json_is_an_error() {
        let out = filtered(TEMPLATE, &Config::default(), true).unwrap();
        let data: Value = serde_json::from_str(&strip_jsonc(&out)).unwrap();
        assert_eq!(data["modules-right"].as_array().unwrap().len(), 4);
        assert!(filtered("{ \"modules-left\": [", &Config::default(), true).is_err());
        assert!(filtered("[1, 2]", &Config::default(), true).is_err());
    }

    #[test]
    fn the_updates_module_only_with_checkupdates() {
        let template = r#"{ "modules-right": ["custom/media", "custom/updates", "custom/power"] }"#;
        let right = |checkupdates, config: &str| {
            let (config, _) = parse_config(config);
            let out = filtered(template, &config, checkupdates).unwrap();
            serde_json::from_str::<Value>(&strip_jsonc(&out)).unwrap()["modules-right"].clone()
        };
        assert_eq!(right(true, ""), serde_json::json!(["custom/media", "custom/updates", "custom/power"]));
        assert_eq!(right(false, ""), serde_json::json!(["custom/media", "custom/power"]));
        assert_eq!(right(true, "[enabled.bar]\nupdates = false\n"), serde_json::json!(["custom/media", "custom/power"]));
        assert_eq!(right(true, "[enabled]\nupdate_notifier = false\n"), serde_json::json!(["custom/media", "custom/power"]));
    }

    #[test]
    fn the_template_of_each_session() {
        let home = Path::new("/nonexistent-home/.config");
        let exe = Path::new("/usr/bin/swaydesk-desktop-config");
        assert_eq!(default_template(true, Some(exe), home), Path::new("/usr/share/swaydesk-desktop/waybar/config.jsonc"));
        // Sem o dos dotfiles e sem o do pacote: o dos dotfiles, para o erro dizer onde se esperava
        assert_eq!(default_template(false, None, home), home.join("waybar/config.jsonc"));
    }
}
