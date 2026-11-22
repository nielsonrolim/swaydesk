//! Os perfis de energia (power-profiles-daemon, pelo `busctl`) e se a máquina tem bateria (pelo `upower`),
//! sem GTK: o popup da bateria e o Sistema usam. O par em Python fica no `sway_power.py`.
use std::process::{Command, Stdio};

/// O serviço, o objeto, a interface e a propriedade do perfil ativo.
pub const PP: [&str; 4] = ["net.hadess.PowerProfiles", "/net/hadess/PowerProfiles", "net.hadess.PowerProfiles", "ActiveProfile"];
/// Os perfis, na ordem dos botões.
pub const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];
/// Os valores do power_profiles_in_system no config.toml.
pub const IN_SYSTEM: &[&str] = &["auto", "always", "never"];

/// A saída de um comando em inglês (LC_ALL=C), com prazo de 3 s (e o KILL 1 s depois, como o timeout
/// do subprocess do Python); vazia se falhou.
pub fn sh(args: &[&str]) -> String {
    Command::new("timeout")
        .args(["-k", "1", "3"])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// A bateria da máquina na lista do `upower -e`: o primeiro caminho com "/battery_", a regra do
/// waybar-battery. Um mouse aparece como .../mouse_hidpp_battery_0 e não conta.
pub fn battery_path(listing: &str) -> Option<String> {
    listing.split_whitespace().find(|p| p.contains("/battery_")).map(str::to_string)
}

/// Se a máquina tem bateria; sem o upower, não.
pub fn has_battery() -> bool {
    battery_path(&sh(&["upower", "-e"])).is_some()
}

/// Se o Sistema mostra os perfis: "always", sempre; "never", nunca; "auto" (e qualquer outro valor), só
/// sem bateria, quando o ícone da bateria some da barra e os botões do popup dela somem junto.
pub fn shows_in_system(setting: &str, has_battery: bool) -> bool {
    match setting {
        "always" => true,
        "never" => false,
        _ => !has_battery,
    }
}

/// A decisão do Sistema ao abrir: só o "auto" (ou um valor desconhecido) pergunta se há bateria.
pub fn decide_in_system(setting: &str, has_battery: impl FnOnce() -> bool) -> bool {
    let battery = !matches!(setting, "always" | "never") && has_battery();
    shows_in_system(setting, battery)
}

/// O `s "balanced"` do `busctl get-property`.
pub fn parse_profile(text: &str) -> String {
    let text = text.trim();
    match text.strip_prefix("s \"").and_then(|t| t.strip_suffix('"')) {
        Some(profile) => profile.to_string(),
        None => String::new(),
    }
}

/// Os perfis que o daemon oferece, do `busctl --json=short get-property ... Profiles` (aa{sv}). O
/// power-profiles-daemon sem suporte da plataforma não tem o performance. Vazio: sem daemon (a seção some).
pub fn parse_profiles(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    value
        .get("data")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("Profile")?.get("data")?.as_str().map(str::to_string))
        .collect()
}

pub fn get_profiles() -> Vec<String> {
    let mut args = vec!["busctl", "--system", "--json=short", "get-property"];
    args.extend(&PP[..3]);
    args.push("Profiles");
    parse_profiles(&sh(&args))
}

pub fn get_profile() -> String {
    let mut args = vec!["busctl", "--system", "get-property"];
    args.extend(PP);
    parse_profile(&sh(&args))
}

/// Fora da linha da interface. Devolve se o perfil mudou.
pub fn set_profile(key: &str) -> bool {
    Command::new("timeout")
        .args(["-k", "1", "35", "busctl", "--system", "--timeout=30", "set-property"])
        .args(PP)
        .args(["s", key])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_profiles_from_busctl() {
        let text = r#"{"type":"aa{sv}","data":[{"Profile":{"type":"s","data":"power-saver"},"Driver":{"type":"s","data":"placeholder"}},{"Profile":{"type":"s","data":"balanced"},"Driver":{"type":"s","data":"placeholder"}}]}"#;
        assert_eq!(parse_profiles(text), ["power-saver", "balanced"]);
        assert!(parse_profiles("").is_empty());
        assert!(parse_profiles("Failed to get property").is_empty());
        assert!(parse_profiles(r#"{"type":"aa{sv}","data":[{"Driver":{"type":"s","data":"x"}}]}"#).is_empty());
    }

    #[test]
    fn profile_from_busctl() {
        assert_eq!(parse_profile("s \"balanced\"\n"), "balanced");
        assert_eq!(parse_profile(""), "");
        assert_eq!(parse_profile("Failed"), "");
    }

    #[test]
    fn the_battery_is_the_machines_not_a_mouses() {
        let mouse = "/org/freedesktop/UPower/devices/line_power_AC\n/org/freedesktop/UPower/devices/mouse_hidpp_battery_0\n\
                     /org/freedesktop/UPower/devices/DisplayDevice\n";
        assert_eq!(battery_path(mouse), None);
        let laptop = format!("{mouse}/org/freedesktop/UPower/devices/battery_BAT0\n");
        assert_eq!(battery_path(&laptop).as_deref(), Some("/org/freedesktop/UPower/devices/battery_BAT0"));
        assert_eq!(battery_path(""), None);
    }

    #[test]
    fn only_auto_asks_for_the_battery() {
        let asked = std::cell::Cell::new(0);
        let ask = |battery: bool| {
            let asked = &asked;
            move || {
                asked.set(asked.get() + 1);
                battery
            }
        };
        assert!(decide_in_system("always", ask(true)));
        assert!(!decide_in_system("never", ask(false)));
        assert_eq!(asked.get(), 0);
        assert!(decide_in_system("auto", ask(false)));
        assert!(!decide_in_system("auto", ask(true)));
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn when_the_system_popup_shows_the_profiles() {
        for (setting, without, with) in [("auto", true, false), ("always", true, true), ("never", false, false), ("sometimes", true, false)] {
            assert_eq!(shows_in_system(setting, false), without, "{setting} sem bateria");
            assert_eq!(shows_in_system(setting, true), with, "{setting} com bateria");
        }
    }
}
