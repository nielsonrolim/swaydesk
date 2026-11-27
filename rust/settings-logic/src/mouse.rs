//! O painel Mouse e touchpad, sem GTK: o que o Sway diz dos dispositivos (o GET_INPUTS, a seção libinput de
//! cada um) e, num Sway puro, se o config dele lê o 80-input.conf.

use crate::input::{Device, Setting};
use std::path::Path;

/// As chaves que o Sway aplica hoje ao primeiro dispositivo do tipo (pointer ou touchpad) do GET_INPUTS, na
/// ordem do painel; None se não há nenhum dispositivo do tipo.
pub fn live(inputs: &serde_json::Value, device: Device) -> Option<Vec<Setting>> {
    let kind = match device {
        Device::Pointer => "pointer",
        Device::Touchpad => "touchpad",
        Device::Keyboard => return None,
    };
    let input = inputs.as_array()?.iter().find(|i| i.get("type").and_then(|t| t.as_str()) == Some(kind))?;
    let libinput = input.get("libinput");
    let field = |name: &str| libinput.and_then(|l| l.get(name));
    let flag = |name: &str| match field(name).and_then(|v| v.as_str()) {
        Some("enabled") => Some(true),
        Some("disabled") => Some(false),
        _ => None,
    };
    let mut out = Vec::new();
    if let Some(speed) = field("accel_speed").and_then(|v| v.as_f64()).filter(|v| v.abs() <= 1.0) {
        out.push(Setting::Accel((speed * 100.0).round() as i32));
    }
    out.extend(flag("natural_scroll").map(Setting::NaturalScroll));
    out.extend(flag("left_handed").map(Setting::LeftHanded));
    if device == Device::Touchpad {
        out.extend(flag("tap").map(Setting::Tap));
        out.extend(flag("dwt").map(Setting::DisableWhileTyping));
    }
    Some(out)
}

/// Se o config do Sway (o texto do GET_CONFIG, o config principal) tem um `include` que pega o arquivo: um
/// caminho igual ou um glob (`*`, `?`) que casa com ele, com `~`, `$HOME`, `$XDG_CONFIG_HOME` (a pasta acima da
/// do config) e as variáveis do próprio config (`set $nome valor`, antes do include) trocadas, e um caminho
/// relativo à pasta do config. Um include de outro arquivo que inclua o nosso não se vê daqui: o painel diz
/// "pode não ser lido", não "não é lido".
pub fn includes(config: &str, file: &Path, home: &Path, config_dir: &Path) -> bool {
    let Some(file) = file.to_str() else { return false };
    let mut vars: Vec<(String, String)> = vec![
        ("$HOME".into(), home.to_string_lossy().into_owned()),
        ("$XDG_CONFIG_HOME".into(), config_dir.parent().unwrap_or(config_dir).to_string_lossy().into_owned()),
    ];
    let expand = |text: &str, vars: &[(String, String)]| {
        let mut sorted: Vec<&(String, String)> = vars.iter().collect();
        // O nome mais longo antes: $cfg_dir não vira o $cfg seguido de "_dir"
        sorted.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        let mut out = text.to_string();
        for (name, value) in sorted {
            out = out.replace(name.as_str(), value);
        }
        out
    };
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((word, rest)) = line.split_once(char::is_whitespace) else { continue };
        let rest = rest.trim();
        match word {
            "set" => {
                if let Some((name, value)) = rest.split_once(char::is_whitespace) {
                    if name.starts_with('$') {
                        let value = expand(value.trim().trim_matches('"'), &vars);
                        // O último set vale, como no Sway
                        match vars.iter_mut().find(|(n, _)| n == name) {
                            Some(old) => old.1 = value,
                            None => vars.push((name.to_string(), value)),
                        }
                    }
                }
            }
            "include" => {
                let path = expand(rest.trim_matches('"'), &vars);
                if path.contains('$') {
                    continue;
                }
                let path = match path.strip_prefix("~/") {
                    Some(tail) => home.join(tail).to_string_lossy().into_owned(),
                    None if path.starts_with('/') => path,
                    None => config_dir.join(&path).to_string_lossy().into_owned(),
                };
                if glob(path.as_bytes(), file.as_bytes()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Um glob simples: `*` (sem passar de uma pasta) e `?`.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            glob(&pattern[1..], text) || (text.first().is_some_and(|&c| c != b'/') && glob(pattern, &text[1..]))
        }
        (Some(b'?'), Some(&c)) if c != b'/' => glob(&pattern[1..], &text[1..]),
        (Some(p), Some(c)) if p == c => glob(&pattern[1..], &text[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> serde_json::Value {
        serde_json::json!([
            {"identifier": "1:1:AT_Translated_Set_2_keyboard", "type": "keyboard"},
            {"identifier": "1739:52804:SYNA_Touchpad", "type": "touchpad", "libinput": {
                "send_events": "enabled", "tap": "enabled", "accel_speed": 0.25, "natural_scroll": "enabled",
                "left_handed": "disabled", "dwt": "enabled"}},
            {"identifier": "1133:16500:Logitech_Mouse", "type": "pointer", "libinput": {
                "accel_speed": -0.5, "natural_scroll": "disabled", "left_handed": "enabled"}},
            {"identifier": "1133:1:Other_Mouse", "type": "pointer", "libinput": {"accel_speed": 1.0}}
        ])
    }

    #[test]
    fn the_live_settings_come_from_the_first_device_of_the_type() {
        assert_eq!(
            live(&inputs(), Device::Pointer),
            Some(vec![Setting::Accel(-50), Setting::NaturalScroll(false), Setting::LeftHanded(true)])
        );
        assert_eq!(
            live(&inputs(), Device::Touchpad),
            Some(vec![
                Setting::Accel(25),
                Setting::NaturalScroll(true),
                Setting::LeftHanded(false),
                Setting::Tap(true),
                Setting::DisableWhileTyping(true),
            ])
        );
        let no_touchpad = serde_json::json!([{"type": "pointer", "libinput": {}}]);
        assert_eq!(live(&no_touchpad, Device::Touchpad), None);
        assert_eq!(live(&no_touchpad, Device::Pointer), Some(vec![]), "a field that's missing is left out");
        assert_eq!(live(&serde_json::json!({}), Device::Pointer), None);
    }

    #[test]
    fn an_include_that_takes_the_file_is_found() {
        let (home, dir) = (Path::new("/home/u"), Path::new("/home/u/.config/sway"));
        let file = Path::new("/home/u/.config/sway/config.d/80-input.conf");
        let found = |config: &str| includes(config, file, home, dir);
        assert!(found("include ~/.config/sway/config.d/*\n"));
        assert!(found("set $mod Mod4\n  include $HOME/.config/sway/config.d/80-*.conf # input\n"));
        assert!(found("include \"/home/u/.config/sway/config.d/80-input.conf\"\n"));
        assert!(found("include config.d/*\n"), "relative to the config's folder");
        assert!(!found("include /etc/sway/config.d/*\n"));
        assert!(!found("# include ~/.config/sway/config.d/*\n"));
        assert!(!found("include ~/.config/sway/config.d/9*\n"));
        assert!(!found(""));
    }

    #[test]
    fn an_include_with_variables_or_extra_spaces_is_found() {
        let (home, dir) = (Path::new("/home/u"), Path::new("/home/u/.config/sway"));
        let file = Path::new("/home/u/.config/sway/config.d/80-input.conf");
        let found = |config: &str| includes(config, file, home, dir);
        assert!(found("include\t ~/.config/sway/config.d/*\n"));
        assert!(found("set $cfg ~/.config/sway\ninclude $cfg/config.d/*\n"));
        assert!(found("set $d /home/u/.config/sway/config.d\ninclude \"$d/*.conf\"\n"));
        assert!(found("include $XDG_CONFIG_HOME/sway/config.d/*\n"), "XDG_CONFIG_HOME is the config's parent");
        assert!(!found("include $cfg/config.d/*\n"), "an unknown variable");
        assert!(found("set $cfg /elsewhere\nset $cfg ~/.config/sway\ninclude $cfg/config.d/*\n"), "the last set wins");
    }
}
