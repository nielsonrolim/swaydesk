//! O arquivo dos painéis Teclado e Mouse e touchpad, `~/.config/sway/config.d/80-input.conf`, sem GTK: só as
//! chaves que o usuário mudou num dos painéis, uma por linha, dentro de um `input type:<tipo> { }` por tipo. Vem
//! depois do 50-local.conf, então vale mais que ele. Cada valor é conferido aqui (uma faixa, um conjunto fixo, um
//! nome XKB só com letras, números e `_-:()`), porque um erro de sintaxe num arquivo do config.d faz o Sway
//! recusar o config inteiro no login. O mesmo texto de uma linha vai ao vivo pelo `swaymsg`.

pub const FILE_NAME: &str = "80-input.conf";

/// O tipo de dispositivo de um bloco (`input type:<tipo>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Device {
    Keyboard,
    Pointer,
    Touchpad,
}

/// Um layout do teclado e a variante dele (vazia: a padrão).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub layout: String,
    pub variant: String,
}

/// Uma chave que os painéis gravam.
#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    /// xkb_layout e xkb_variant juntos, na ordem da troca.
    Layouts(Vec<Layout>),
    /// xkb_options inteiro (o painel só mexe nas grp:, e o resto vem de onde estava).
    Options(Vec<String>),
    /// repeat_delay, em ms (100 a 2000).
    RepeatDelay(u32),
    /// repeat_rate, por segundo (1 a 100).
    RepeatRate(u32),
    /// xkb_numlock: o Num Lock ligado quando o teclado aparece (no login).
    NumLock(bool),
    /// pointer_accel, em centésimos (-100 a 100).
    Accel(i32),
    NaturalScroll(bool),
    LeftHanded(bool),
    Tap(bool),
    /// dwt: o touchpad desliga enquanto se digita.
    DisableWhileTyping(bool),
}

/// O conteúdo do arquivo: as chaves de cada tipo, na ordem em que foram dadas, uma de cada.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputConf {
    pub keyboard: Vec<Setting>,
    pub pointer: Vec<Setting>,
    pub touchpad: Vec<Setting>,
}

impl Setting {
    /// As linhas do bloco (o que vem depois de `input type:<tipo>`): `xkb_layout "br,us"` e `xkb_variant ","`
    /// para os layouts, uma para o resto; ou o erro de um valor fora do que o Sway aceita.
    pub fn sway(&self) -> Result<Vec<String>, String> {
        let on = |b: bool| if b { "enabled" } else { "disabled" };
        Ok(vec![match self {
            Setting::Layouts(list) => {
                if list.is_empty() {
                    return Err("no keyboard layout".into());
                }
                for item in list {
                    if !xkb_name(&item.layout) || !(item.variant.is_empty() || xkb_name(&item.variant)) {
                        return Err(format!("not a keyboard layout: {:?} {:?}", item.layout, item.variant));
                    }
                }
                let join = |f: fn(&Layout) -> &str| list.iter().map(f).collect::<Vec<_>>().join(",");
                return Ok(vec![
                    format!("xkb_layout \"{}\"", join(|l| &l.layout)),
                    format!("xkb_variant \"{}\"", join(|l| &l.variant)),
                ]);
            }
            Setting::Options(options) => {
                if let Some(bad) = options.iter().find(|o| !xkb_name(o)) {
                    return Err(format!("not a keyboard option: {bad:?}"));
                }
                format!("xkb_options \"{}\"", options.join(","))
            }
            Setting::RepeatDelay(ms @ 100..=2000) => format!("repeat_delay {ms}"),
            Setting::RepeatRate(rate @ 1..=100) => format!("repeat_rate {rate}"),
            Setting::NumLock(b) => format!("xkb_numlock {}", on(*b)),
            Setting::Accel(accel @ -100..=100) => format!("pointer_accel {}", f64::from(*accel) / 100.0),
            Setting::NaturalScroll(b) => format!("natural_scroll {}", on(*b)),
            Setting::LeftHanded(b) => format!("left_handed {}", on(*b)),
            Setting::Tap(b) => format!("tap {}", on(*b)),
            Setting::DisableWhileTyping(b) => format!("dwt {}", on(*b)),
            Setting::RepeatDelay(_) | Setting::RepeatRate(_) | Setting::Accel(_) => {
                return Err(format!("out of range: {self:?}"));
            }
        }])
    }

    /// O nome da chave no Sway (a primeira palavra da linha), o mesmo para valores diferentes.
    pub fn key(&self) -> &'static str {
        match self {
            Setting::Layouts(_) => "xkb_layout",
            Setting::Options(_) => "xkb_options",
            Setting::RepeatDelay(_) => "repeat_delay",
            Setting::RepeatRate(_) => "repeat_rate",
            Setting::NumLock(_) => "xkb_numlock",
            Setting::Accel(_) => "pointer_accel",
            Setting::NaturalScroll(_) => "natural_scroll",
            Setting::LeftHanded(_) => "left_handed",
            Setting::Tap(_) => "tap",
            Setting::DisableWhileTyping(_) => "dwt",
        }
    }
}

/// Um nome do xkeyboard-config (layout, variante, opção): só letras e números ASCII e `_-:()`. Nada que feche
/// as aspas, separe comandos (`;` `,`), troque uma variável (`$`) ou comece um comentário.
pub(crate) fn xkb_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '(' | ')'))
}

impl Device {
    /// As chaves do Sway que os painéis gravam para o tipo.
    fn keys(self) -> &'static [&'static str] {
        match self {
            Device::Keyboard => {
                &["xkb_layout", "xkb_variant", "xkb_options", "repeat_delay", "repeat_rate", "xkb_numlock"]
            }
            Device::Pointer => &["pointer_accel", "natural_scroll", "left_handed"],
            Device::Touchpad => &["pointer_accel", "natural_scroll", "left_handed", "tap", "dwt"],
        }
    }

    fn criteria(self) -> &'static str {
        match self {
            Device::Keyboard => "type:keyboard",
            Device::Pointer => "type:pointer",
            Device::Touchpad => "type:touchpad",
        }
    }
}

/// O comando do `swaymsg` que põe as chaves pedidas (tipo, nome da chave do Sway) como os arquivos dizem
/// (`effective`, com o config principal e todo o config.d, o nosso incluído); uma chave que nenhum arquivo dá volta
/// ao padrão: o do Sway e do libinput, e para o teclado o do ambiente (XKB_DEFAULT_LAYOUT, _VARIANT, _OPTIONS,
/// que o Sway usa sem xkb_layout num arquivo), senão o us. O tap, o dwt e o Num Lock não têm padrão fixo (o
/// touchpad decide): sem arquivo, ficam como estão. O Num Lock nunca vai: o Sway o recusa fora do config.
///
/// Só as chaves pedidas: um xkb_layout ou xkb_variant recompila o keymap, e o Sway volta ao primeiro layout, então
/// uma mudança do mouse não manda nada do teclado. Mandar duas vezes dá no mesmo: uma mudança feita enquanto o
/// comando anterior roda só junta as chaves dela às pendentes. O "Usar o teclado do sistema" tira os layouts do
/// nosso arquivo e põe os do 50-local.conf de volta sem um reload.
pub fn apply(effective: &InputConf, keys: &[(Device, &str)], env: impl Fn(&str) -> Option<String>) -> String {
    let mut out = Vec::new();
    for &(device, key) in keys {
        // O Sway só aceita o xkb_numlock no config ("Can only be used in config file"): vale no próximo login
        if key == "xkb_numlock" {
            continue;
        }
        let given = effective.list(device).iter().find(|s| s.key() == key).cloned();
        let Some(setting) = given.or_else(|| default(device, key, &env)) else { continue };
        if let Ok(command) = command(device, &setting) {
            out.push(command);
        }
    }
    out.join("; ")
}

/// Os layouts que valem: os dos arquivos e, sem nenhum, os do ambiente (XKB_DEFAULT_LAYOUT, que um Sway puro pode
/// exportar) e o us. É o que o painel mostra como o teclado do sistema e de onde parte ao adicionar um layout.
pub fn system_layouts(effective: &InputConf, env: impl Fn(&str) -> Option<String>) -> Vec<Layout> {
    let given = effective.list(Device::Keyboard).iter().find(|s| s.key() == "xkb_layout").cloned();
    match given.or_else(|| default(Device::Keyboard, "xkb_layout", &env)) {
        Some(Setting::Layouts(list)) => list,
        _ => vec![Layout { layout: "us".into(), variant: String::new() }],
    }
}

/// O valor de uma chave sem nenhum arquivo que a dê.
fn default(device: Device, key: &str, env: &impl Fn(&str) -> Option<String>) -> Option<Setting> {
    let valid = |setting: Setting| setting.sway().is_ok().then_some(setting);
    Some(match (device, key) {
        (Device::Keyboard, "xkb_layout") => {
            let from_env = env("XKB_DEFAULT_LAYOUT")
                .filter(|l| !l.is_empty())
                .and_then(|l| valid(layouts_of(&l, env("XKB_DEFAULT_VARIANT").as_deref())));
            from_env.unwrap_or_else(|| Setting::Layouts(vec![Layout { layout: "us".into(), variant: String::new() }]))
        }
        (Device::Keyboard, "xkb_options") => {
            let from_env = env("XKB_DEFAULT_OPTIONS")
                .filter(|o| !o.is_empty())
                .and_then(|o| valid(Setting::Options(o.split(',').map(str::to_string).collect())));
            from_env.unwrap_or(Setting::Options(Vec::new()))
        }
        (Device::Keyboard, "repeat_delay") => Setting::RepeatDelay(600),
        (Device::Keyboard, "repeat_rate") => Setting::RepeatRate(25),
        (Device::Pointer | Device::Touchpad, "pointer_accel") => Setting::Accel(0),
        (Device::Pointer | Device::Touchpad, "natural_scroll") => Setting::NaturalScroll(false),
        (Device::Pointer | Device::Touchpad, "left_handed") => Setting::LeftHanded(false),
        _ => return None,
    })
}

/// O comando do `swaymsg` que aplica a chave ao vivo (um texto só, que o swaymsg passa inteiro ao Sway).
pub fn command(device: Device, setting: &Setting) -> Result<String, String> {
    let lines = setting.sway()?;
    Ok(lines.iter().map(|line| format!("input {} {line}", device.criteria())).collect::<Vec<_>>().join("; "))
}

impl InputConf {
    /// Troca (ou acrescenta) a chave do mesmo tipo.
    pub fn set(&mut self, device: Device, setting: Setting) {
        let list = self.list_mut(device);
        match list.iter_mut().find(|s| s.key() == setting.key()) {
            Some(old) => *old = setting,
            None => list.push(setting),
        }
    }

    /// Tira a chave do mesmo tipo que `like` (o valor de `like` não conta).
    pub fn unset(&mut self, device: Device, like: &Setting) {
        self.list_mut(device).retain(|s| s.key() != like.key());
    }

    fn list_mut(&mut self, device: Device) -> &mut Vec<Setting> {
        match device {
            Device::Keyboard => &mut self.keyboard,
            Device::Pointer => &mut self.pointer,
            Device::Touchpad => &mut self.touchpad,
        }
    }

    /// As chaves de um tipo.
    pub fn list(&self, device: Device) -> &[Setting] {
        match device {
            Device::Keyboard => &self.keyboard,
            Device::Pointer => &self.pointer,
            Device::Touchpad => &self.touchpad,
        }
    }

    fn blocks(&self) -> [(Device, &Vec<Setting>); 3] {
        [(Device::Keyboard, &self.keyboard), (Device::Pointer, &self.pointer), (Device::Touchpad, &self.touchpad)]
    }

    /// O texto do arquivo; uma chave inválida fica de fora (a gravação nunca leva um erro ao login).
    pub fn render(&self) -> String {
        let mut text = String::from(
            // Em inglês: o arquivo vai para todo usuário, em qualquer língua
            "# Generated by the Keyboard and Mouse and touchpad panels of Settings (swaydesk-settings): change it\n\
             # there, not by hand, or the next save undoes it. Read after 50-local.conf, so it wins over it.\n",
        );
        for (device, settings) in self.blocks() {
            let lines: Vec<String> = settings.iter().filter_map(|s| s.sway().ok()).flatten().collect();
            if lines.is_empty() {
                continue;
            }
            text.push_str(&format!("input {} {{\n", device.criteria()));
            for line in lines {
                text.push_str(&format!("    {line}\n"));
            }
            text.push_str("}\n");
        }
        text
    }

    /// As chaves que valem na sessão: as dos arquivos do config.d, na ordem do `include` (a última vale, como no
    /// Sway), nos blocos `input type:<tipo>` e `input *` (um bloco de um dispositivo só fica de fora).
    pub fn effective(confs: &[String]) -> InputConf {
        let mut conf = InputConf::default();
        for text in confs {
            conf.read(text);
        }
        conf
    }

    /// Os dispositivos com um bloco só deles (`input "<identificador>"`) que dá uma chave de um dos tipos do painel
    /// (`devices`: o teclado no Teclado, o mouse e o touchpad no Mouse e touchpad): valem mais que o painel para
    /// aquele dispositivo, e o painel avisa. Um `type:` que o painel não conhece (type:tablet_tool) é um tipo, não um
    /// dispositivo. Na ordem em que aparecem, sem repetir.
    pub fn device_specific(confs: &[String], devices: &[Device]) -> Vec<String> {
        let keys: Vec<&str> = devices.iter().flat_map(|d| d.keys().iter().copied()).collect();
        let mut out: Vec<String> = Vec::new();
        for (criteria, statements) in confs.iter().flat_map(|text| blocks(text)) {
            let one_device = criteria != "*" && !criteria.starts_with("type:");
            if one_device && statements.iter().any(|(k, _)| keys.contains(&k.as_str())) && !out.contains(&criteria) {
                out.push(criteria);
            }
        }
        out
    }

    /// Lê o arquivo que o `render` gravou; o que não for uma chave válida de um bloco nosso fica de fora.
    pub fn parse(text: &str) -> InputConf {
        let mut conf = InputConf::default();
        conf.read(text);
        conf
    }

    /// Junta as chaves válidas de um texto do config do Sway às que já tem.
    fn read(&mut self, text: &str) {
        for (criteria, statements) in blocks(text) {
            let devices = devices_of(&criteria);
            for (key, value) in &statements {
                let setting = |device| match key.as_str() {
                    "xkb_variant" => None,
                    // O xkb_variant do bloco, onde estiver (o último vale). Um xkb_layout sem variante num bloco
                    // volta às padrão aqui, mas o Sway junta campo a campo e manteria a de um bloco anterior: só
                    // conta num arquivo feito à mão, porque o render grava os dois juntos
                    "xkb_layout" => {
                        let variants =
                            statements.iter().rev().find(|(k, _)| k == "xkb_variant").map(|(_, v)| v.as_str());
                        (device == Device::Keyboard).then(|| layouts_of(value, variants))
                    }
                    _ => setting_of(key, value, device),
                };
                for &device in &devices {
                    if let Some(setting) = setting(device).filter(|s| s.sway().is_ok()) {
                        self.set(device, setting);
                    }
                }
            }
        }
    }
}

/// Os blocos `input` de um texto do config do Sway: o critério (sem as aspas) e as linhas `chave valor` (o valor
/// sem as aspas). As três formas do Sway: `input <c> {` … `}`, a chave na linha de baixo, e `input <c> chave valor`.
fn blocks(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut opening: Option<String> = None;
    let mut inside: Option<(String, Vec<(String, String)>)> = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let was_opening = opening.take();
        if line.is_empty() {
            opening = was_opening;
            continue;
        }
        if let Some((criteria, statements)) = inside.as_mut() {
            if line == "}" {
                out.push((std::mem::take(criteria), std::mem::take(statements)));
                inside = None;
            } else if let Some(statement) = statement(line) {
                statements.push(statement);
            }
            continue;
        }
        if line == "{" {
            if let Some(criteria) = was_opening {
                inside = Some((criteria, Vec::new()));
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("input ") else { continue };
        let rest = rest.trim();
        let (criteria, tail) = match rest.strip_prefix('"') {
            Some(quoted) => match quoted.split_once('"') {
                Some((c, t)) => (c, t.trim()),
                None => continue,
            },
            None => rest.split_once(char::is_whitespace).map_or((rest, ""), |(c, t)| (c, t.trim())),
        };
        let criteria = criteria.to_string();
        match tail {
            "" => opening = Some(criteria),
            "{" => inside = Some((criteria, Vec::new())),
            _ => out.push((criteria, statement(tail).into_iter().collect())),
        }
    }
    out
}

/// Os tipos de um critério do `input`: um tipo, todos (`*`) ou nenhum (um dispositivo só, pelo identificador).
fn devices_of(criteria: &str) -> Vec<Device> {
    match criteria {
        "*" => vec![Device::Keyboard, Device::Pointer, Device::Touchpad],
        "type:keyboard" => vec![Device::Keyboard],
        "type:pointer" => vec![Device::Pointer],
        "type:touchpad" => vec![Device::Touchpad],
        _ => Vec::new(),
    }
}

/// `chave valor` de uma linha, o valor sem as aspas em volta.
fn statement(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(char::is_whitespace)?;
    let value = value.trim();
    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
    Some((key.to_string(), value.to_string()))
}

/// Os layouts de um xkb_layout e do xkb_variant do mesmo bloco (sem ele, ou com outra contagem, as variantes
/// padrão).
fn layouts_of(layouts: &str, variants: Option<&str>) -> Setting {
    let names: Vec<&str> = layouts.split(',').collect();
    let variants: Vec<&str> = variants
        .map(|v| v.split(',').collect())
        .filter(|v: &Vec<&str>| v.len() == names.len())
        .unwrap_or_else(|| vec![""; names.len()]);
    let pair = |(l, v): (&&str, &str)| Layout { layout: l.trim().to_string(), variant: v.trim().to_string() };
    Setting::Layouts(names.iter().zip(variants).map(pair).collect())
}

/// A chave de uma linha, se ela vale para o tipo.
fn setting_of(key: &str, value: &str, device: Device) -> Option<Setting> {
    let flag = match value {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    };
    let pointing = matches!(device, Device::Pointer | Device::Touchpad);
    match key {
        "xkb_options" if device == Device::Keyboard => {
            let options = if value.is_empty() { vec![] } else { value.split(',').map(str::to_string).collect() };
            Some(Setting::Options(options))
        }
        "repeat_delay" if device == Device::Keyboard => value.parse().ok().map(Setting::RepeatDelay),
        "repeat_rate" if device == Device::Keyboard => value.parse().ok().map(Setting::RepeatRate),
        "xkb_numlock" if device == Device::Keyboard => flag.map(Setting::NumLock),
        "pointer_accel" if pointing => value
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && v.abs() <= 1.0)
            .map(|v| Setting::Accel((v * 100.0).round() as i32)),
        "natural_scroll" if pointing => flag.map(Setting::NaturalScroll),
        "left_handed" if pointing => flag.map(Setting::LeftHanded),
        "tap" if device == Device::Touchpad => flag.map(Setting::Tap),
        "dwt" if device == Device::Touchpad => flag.map(Setting::DisableWhileTyping),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layouts(list: &[(&str, &str)]) -> Setting {
        Setting::Layouts(list.iter().map(|(l, v)| Layout { layout: l.to_string(), variant: v.to_string() }).collect())
    }

    fn every_setting() -> InputConf {
        InputConf {
            keyboard: vec![
                layouts(&[("br", "abnt2"), ("us", "dvorak-alt-intl"), ("de", ""), ("ru", "phonetic_YAZHERTY")]),
                Setting::Options(vec!["grp:alt_shift_toggle".into(), "compose:ralt".into(), "lv3:ralt_switch".into()]),
                Setting::RepeatDelay(250),
                Setting::RepeatRate(40),
                Setting::NumLock(true),
            ],
            pointer: vec![Setting::Accel(-35), Setting::NaturalScroll(false), Setting::LeftHanded(true)],
            touchpad: vec![
                Setting::Accel(100),
                Setting::NaturalScroll(true),
                Setting::Tap(false),
                Setting::DisableWhileTyping(true),
            ],
        }
    }

    fn line(setting: Setting) -> String {
        setting.sway().unwrap().join("\n")
    }

    #[test]
    fn each_setting_is_one_sway_line() {
        assert_eq!(line(layouts(&[("br", "abnt2"), ("us", "")])), "xkb_layout \"br,us\"\nxkb_variant \"abnt2,\"");
        assert_eq!(line(Setting::Options(vec![])), "xkb_options \"\"");
        assert_eq!(line(Setting::RepeatDelay(600)), "repeat_delay 600");
        assert_eq!(line(Setting::RepeatRate(25)), "repeat_rate 25");
        assert_eq!(line(Setting::NumLock(false)), "xkb_numlock disabled");
        assert_eq!(line(Setting::Accel(-35)), "pointer_accel -0.35");
        assert_eq!(line(Setting::Accel(0)), "pointer_accel 0");
        assert_eq!(line(Setting::Accel(100)), "pointer_accel 1");
        assert_eq!(line(Setting::NaturalScroll(true)), "natural_scroll enabled");
        assert_eq!(line(Setting::LeftHanded(false)), "left_handed disabled");
        assert_eq!(line(Setting::Tap(true)), "tap enabled");
        assert_eq!(line(Setting::DisableWhileTyping(false)), "dwt disabled");
    }

    #[test]
    fn values_outside_what_sway_takes_are_refused() {
        assert!(Setting::RepeatDelay(99).sway().is_err());
        assert!(Setting::RepeatDelay(2001).sway().is_err());
        assert!(Setting::RepeatRate(0).sway().is_err());
        assert!(Setting::RepeatRate(101).sway().is_err());
        assert!(Setting::Accel(101).sway().is_err());
        assert!(Setting::Accel(-101).sway().is_err());
        assert!(layouts(&[]).sway().is_err(), "at least one layout");
        assert!(layouts(&[("", "x")]).sway().is_err(), "a layout name");
        for bad in ["br\"", "br;exec x", "us,de", "us de", "$x", "br}", "é", "a\nb", "us#x"] {
            assert!(layouts(&[(bad, "")]).sway().is_err(), "layout {bad:?}");
            assert!(layouts(&[("us", bad)]).sway().is_err(), "variant {bad:?}");
            assert!(Setting::Options(vec![bad.into()]).sway().is_err(), "option {bad:?}");
        }
        assert!(Setting::Options(vec!["".into()]).sway().is_err());
        // Nomes do xkeyboard-config que têm de passar
        assert!(layouts(&[("us", "dvorak-alt-intl"), ("fr", "oss_latin9"), ("ru", "phonetic_YAZHERTY")]).sway().is_ok());
        assert!(Setting::Options(vec!["lv3:ralt_switch".into(), "grp:alt_shift_toggle".into()]).sway().is_ok());
    }

    #[test]
    fn the_live_command_names_the_device_type() {
        assert_eq!(command(Device::Touchpad, &Setting::Tap(true)).unwrap(), "input type:touchpad tap enabled");
        assert_eq!(
            command(Device::Keyboard, &layouts(&[("br", ""), ("us", "intl")])).unwrap(),
            "input type:keyboard xkb_layout \"br,us\"; input type:keyboard xkb_variant \",intl\""
        );
        assert!(command(Device::Keyboard, &Setting::RepeatRate(0)).is_err());
    }

    #[test]
    fn set_replaces_the_same_kind_and_unset_removes_it() {
        let mut conf = InputConf::default();
        conf.set(Device::Pointer, Setting::Accel(10));
        conf.set(Device::Pointer, Setting::LeftHanded(true));
        conf.set(Device::Pointer, Setting::Accel(-20));
        assert_eq!(conf.pointer, vec![Setting::Accel(-20), Setting::LeftHanded(true)]);
        conf.unset(Device::Pointer, &Setting::Accel(0));
        assert_eq!(conf.pointer, vec![Setting::LeftHanded(true)]);
        assert!(conf.keyboard.is_empty() && conf.touchpad.is_empty());
    }

    #[test]
    fn the_file_has_one_block_per_type_with_settings() {
        let mut conf = InputConf::default();
        assert_eq!(conf.render().lines().filter(|l| !l.starts_with('#')).count(), 0, "nothing but the header");
        conf.set(Device::Touchpad, Setting::Tap(false));
        conf.set(Device::Touchpad, Setting::RepeatRate(0)); // inválida: fica de fora
        let text = conf.render();
        let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(body, vec!["input type:touchpad {", "    tap disabled", "}"]);
    }

    #[test]
    fn the_file_reads_back_what_was_written() {
        let conf = every_setting();
        assert_eq!(InputConf::parse(&conf.render()), conf);
    }

    #[test]
    fn a_hand_edited_file_keeps_only_valid_settings() {
        let text = "input type:keyboard {\n    repeat_rate 500\n    repeat_delay 300\n    exec rm -rf ~\n}\n\
                    input type:pointer {\n    pointer_accel 0.5\n    pointer_accel abc\n}\n\
                    input \"1:1:Mouse\" {\n    left_handed enabled\n}\n";
        let conf = InputConf::parse(text);
        assert_eq!(conf.keyboard, vec![Setting::RepeatDelay(300)]);
        assert_eq!(conf.pointer, vec![Setting::Accel(50)]);
        assert!(conf.touchpad.is_empty());
    }

    /// Os dois arquivos que o headless-settings.sh passa pelo `sway -C` (o Sway recusaria o config inteiro no
    /// login por um erro de sintaxe): toda chave com nomes XKB variados, e os valores vazios (variantes padrão,
    /// nenhuma opção). Um render diferente quebra este teste, e os arquivos se regeneram com
    /// SWAYDESK_BLESS=1.
    #[test]
    fn the_files_checked_by_sway_are_what_render_writes() {
        let mut empty = InputConf::default();
        empty.set(Device::Keyboard, layouts(&[("us", ""), ("br", "")]));
        empty.set(Device::Keyboard, Setting::Options(vec![]));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        for (name, conf) in [("80-input-every-key.conf", every_setting()), ("80-input-empty-values.conf", empty)] {
            let file = dir.join(name);
            if std::env::var_os("SWAYDESK_BLESS").is_some() {
                std::fs::write(&file, conf.render()).unwrap();
            }
            let saved = std::fs::read_to_string(&file).unwrap_or_default();
            assert_eq!(saved, conf.render(), "{} is not what render() writes", file.display());
        }
    }

    #[test]
    fn the_session_keyboard_comes_from_every_config_d_file_in_order() {
        // O bloco do teclado do sistema que o setup grava no 50-local.conf: sem aspas e alinhado
        let local = "# --- Teclado do sistema (/etc/default/keyboard), lido na instalação ---\n\
                     input type:keyboard {\n    xkb_model    pc105\n    xkb_layout   br,us\n    xkb_variant  abnt2,intl\n}\n\
                     input * {\n    repeat_delay 300 # comentário\n}\n\
                     input type:keyboard xkb_options compose:ralt\n\
                     input type:keyboard\n{\n    xkb_numlock enabled\n}\n";
        let ours = "input type:keyboard {\n    repeat_rate 40\n}\n";
        let conf = InputConf::effective(&[local.to_string(), ours.to_string()]);
        assert_eq!(
            conf.keyboard,
            vec![
                layouts(&[("br", "abnt2"), ("us", "intl")]),
                Setting::RepeatDelay(300),
                Setting::Options(vec!["compose:ralt".into()]),
                Setting::NumLock(true),
                Setting::RepeatRate(40),
            ]
        );
        // O último vale, como no Sway
        let later = "input type:keyboard {\n    xkb_layout \"de\"\n    repeat_delay 500\n}\n";
        let conf = InputConf::effective(&[local.to_string(), later.to_string()]);
        assert_eq!(conf.keyboard[0], layouts(&[("de", "")]));
        assert_eq!(conf.keyboard[1], Setting::RepeatDelay(500));
    }

    #[test]
    fn the_header_is_in_english_and_an_old_header_still_reads() {
        assert!(InputConf::default().render().starts_with("# Generated by the Keyboard and Mouse and touchpad panels"));
        let old = "# Gerado pelos painéis Teclado e Mouse e touchpad das Configurações (swaydesk-settings): mude por\n\
                   # eles, não à mão, ou a próxima gravação desfaz.\ninput type:pointer {\n    left_handed enabled\n}\n";
        assert_eq!(InputConf::parse(old).pointer, vec![Setting::LeftHanded(true)]);
    }

    #[test]
    fn the_variant_of_a_block_counts_wherever_it_is() {
        let before = "input type:keyboard {\n    xkb_variant abnt2,intl\n    xkb_layout br,us\n}\n";
        assert_eq!(InputConf::effective(&[before.into()]).keyboard, vec![layouts(&[("br", "abnt2"), ("us", "intl")])]);
        let twice = "input type:keyboard {\n    xkb_layout br\n    xkb_variant nodeadkeys\n    xkb_variant abnt2\n}\n";
        assert_eq!(InputConf::effective(&[twice.into()]).keyboard, vec![layouts(&[("br", "abnt2")])]);
    }

    #[test]
    fn a_block_for_one_device_does_not_count() {
        let text = "input \"1:1:AT_Translated_Set_2_keyboard\" {\n    xkb_layout fr\n}\ninput type:touchpad tap enabled\n";
        let conf = InputConf::effective(&[text.to_string()]);
        assert!(conf.keyboard.is_empty());
        assert_eq!(conf.touchpad, vec![Setting::Tap(true)]);
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string())
    }

    #[test]
    fn apply_sends_only_the_asked_keys_with_defaults_for_the_missing() {
        let mut conf = InputConf::default();
        conf.set(Device::Keyboard, layouts(&[("br", "abnt2")]));
        conf.set(Device::Touchpad, Setting::Tap(true));
        conf.set(Device::Pointer, Setting::Accel(-20));
        let keys = [
            (Device::Pointer, "pointer_accel"),
            (Device::Pointer, "natural_scroll"),
            (Device::Touchpad, "tap"),
            (Device::Touchpad, "dwt"),
            (Device::Keyboard, "repeat_delay"),
        ];
        let commands: Vec<String> = apply(&conf, &keys, env(&[])).split("; ").map(str::to_string).collect();
        assert_eq!(
            commands,
            vec![
                "input type:pointer pointer_accel -0.2",
                "input type:pointer natural_scroll disabled",
                "input type:touchpad tap enabled",
                "input type:keyboard repeat_delay 600",
            ],
            "dwt has no fixed default: left alone"
        );
        // A mudança do mouse não manda xkb_ (recompilar o keymap volta ao primeiro layout)
        assert!(!apply(&conf, &keys[..4], env(&[])).contains("xkb_"));
        assert_eq!(apply(&conf, &[], env(&[])), "");
        // O Sway recusa o xkb_numlock fora do config ("Can only be used in config file"): vale no login
        conf.set(Device::Keyboard, Setting::NumLock(true));
        assert_eq!(apply(&conf, &[(Device::Keyboard, "xkb_numlock")], env(&[])), "");
    }

    #[test]
    fn the_system_layouts_are_the_files_then_the_xkb_default_environment_then_us() {
        let none = InputConf::default();
        assert_eq!(system_layouts(&none, env(&[])), vec![Layout { layout: "us".into(), variant: String::new() }]);
        let xkb = env(&[("XKB_DEFAULT_LAYOUT", "de,br"), ("XKB_DEFAULT_VARIANT", ",abnt2")]);
        assert_eq!(
            system_layouts(&none, xkb),
            vec![Layout { layout: "de".into(), variant: String::new() }, Layout { layout: "br".into(), variant: "abnt2".into() }]
        );
        // Um arquivo (o 50-local do instalador) vence o ambiente
        let mut conf = InputConf::default();
        conf.set(Device::Keyboard, layouts(&[("fr", "")]));
        assert_eq!(system_layouts(&conf, env(&[("XKB_DEFAULT_LAYOUT", "de")])), vec![Layout { layout: "fr".into(), variant: String::new() }]);
    }

    #[test]
    fn the_layout_default_is_the_xkb_default_environment_then_us() {
        let keys = [(Device::Keyboard, "xkb_layout"), (Device::Keyboard, "xkb_options")];
        let none = InputConf::default();
        assert_eq!(
            apply(&none, &keys, env(&[])),
            "input type:keyboard xkb_layout \"us\"; input type:keyboard xkb_variant \"\"; input type:keyboard xkb_options \"\""
        );
        let xkb = env(&[("XKB_DEFAULT_LAYOUT", "br,us"), ("XKB_DEFAULT_VARIANT", "abnt2,intl"), ("XKB_DEFAULT_OPTIONS", "compose:ralt")]);
        assert_eq!(
            apply(&none, &keys, xkb),
            "input type:keyboard xkb_layout \"br,us\"; input type:keyboard xkb_variant \"abnt2,intl\"; \
             input type:keyboard xkb_options \"compose:ralt\""
        );
        let bad = env(&[("XKB_DEFAULT_LAYOUT", "br;exec x"), ("XKB_DEFAULT_OPTIONS", "a b")]);
        assert_eq!(
            apply(&none, &keys, bad),
            "input type:keyboard xkb_layout \"us\"; input type:keyboard xkb_variant \"\"; input type:keyboard xkb_options \"\""
        );
        // Um arquivo que dá a chave vence o ambiente
        let mut conf = InputConf::default();
        conf.set(Device::Keyboard, layouts(&[("de", "")]));
        assert!(apply(&conf, &keys[..1], env(&[("XKB_DEFAULT_LAYOUT", "br")])).contains("xkb_layout \"de\""));
    }

    #[test]
    fn a_device_block_with_a_key_of_the_panel_is_named() {
        let confs = [
            "input \"1739:52804:SYNA_Touchpad\" {\n    tap disabled\n}\n\
             input 1:1:AT_Translated_Set_2_keyboard xkb_layout fr\n\
             input \"1133:1:Mouse\" {\n    scroll_factor 2\n}\n"
                .to_string(),
            "input type:pointer accel_profile flat\ninput \"1739:52804:SYNA_Touchpad\" dwt disabled\n\
             input type:tablet_tool left_handed enabled\n"
                .to_string(),
        ];
        assert_eq!(
            InputConf::device_specific(&confs, &[Device::Keyboard]),
            vec!["1:1:AT_Translated_Set_2_keyboard".to_string()],
            "the Keyboard panel names only keyboard keys"
        );
        assert_eq!(
            InputConf::device_specific(&confs, &[Device::Pointer, Device::Touchpad]),
            vec!["1739:52804:SYNA_Touchpad".to_string()],
            "scroll_factor isn't a panel key, and type:tablet_tool is a type, not one device"
        );
        assert!(InputConf::device_specific(&["input type:keyboard xkb_layout br\n".into()], &[Device::Keyboard]).is_empty());
    }
}
