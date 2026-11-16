//! As saídas do `swaymsg -r -t get_outputs`: o nome da porta, a identidade do monitor (fabricante, modelo e
//! número de série, como o Sway aceita em `output "<id>"`), os modos e o estado atual.
use serde_json::Value;

use crate::config::{OutputConfig, Transform};

/// Um modo da saída. A taxa vem em mHz, como no Sway (60.008 Hz = 60008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
}

impl Mode {
    /// "1920x1080@60.008Hz", o formato do comando `output … mode`. Sem taxa (saídas virtuais), só o tamanho
    pub fn command(&self) -> String {
        if self.refresh <= 0 {
            return format!("{}x{}", self.width, self.height);
        }
        format!("{}x{}@{}.{:03}Hz", self.width, self.height, self.refresh / 1000, self.refresh % 1000)
    }

    /// "60 Hz", "143,91 Hz": a taxa para mostrar, sem zeros sobrando
    pub fn refresh_label(&self) -> String {
        let hz = self.refresh as f64 / 1000.0;
        let text = format!("{hz:.2}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        format!("{text} Hz")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// O nome da porta: "eDP-1", "HDMI-A-1"
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    /// Ligada (o Sway desenha nela)
    pub active: bool,
    pub modes: Vec<Mode>,
    pub current: Option<Mode>,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: Transform,
    pub adaptive_sync: bool,
    /// O tamanho físico em mm, para o modo preferido no rótulo
    pub physical: (i32, i32),
    /// A saída com o foco do Sway (onde a pessoa está)
    pub focused: bool,
}

/// As telas de notebook: a interna some com a tampa fechada
pub fn is_internal(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|prefix| name.starts_with(prefix))
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn number(value: &Value, key: &str) -> i32 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}

fn mode(value: &Value) -> Option<Mode> {
    let mode = Mode { width: number(value, "width"), height: number(value, "height"), refresh: number(value, "refresh") };
    (mode.width > 0 && mode.height > 0).then_some(mode)
}

impl Output {
    /// "Fabricante Modelo Serial", o identificador do Sway. Campo vazio vira "Unknown", como no Sway.
    pub fn identity(&self) -> String {
        let field = |s: &str| if s.is_empty() { "Unknown".to_string() } else { s.to_string() };
        format!("{} {} {}", field(&self.make), field(&self.model), field(&self.serial))
    }

    /// "Fabricante Modelo", para o rótulo
    pub fn description(&self) -> String {
        let parts: Vec<&str> =
            [self.make.as_str(), self.model.as_str()].into_iter().filter(|s| !s.is_empty() && *s != "Unknown").collect();
        parts.join(" ")
    }

    /// A configuração em uso, como o perfil a guardaria. Desligada, guarda o modo preferido.
    pub fn config(&self) -> OutputConfig {
        let mode = self.current.or_else(|| self.modes.iter().max().copied());
        OutputConfig {
            enabled: self.active,
            mode,
            x: self.x,
            y: self.y,
            scale: if self.scale > 0.0 { self.scale } else { 1.0 },
            transform: self.transform,
            adaptive_sync: self.adaptive_sync,
        }
    }
}

/// As saídas do JSON do get_outputs, na ordem do Sway. Ignora as "non_desktop" (óculos de VR).
pub fn parse(outputs: &Value) -> Vec<Output> {
    let mut list = Vec::new();
    for item in outputs.as_array().into_iter().flatten() {
        let name = text(item, "name");
        if name.is_empty() || item.get("non_desktop").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let mut modes: Vec<Mode> = item.get("modes").and_then(Value::as_array).into_iter().flatten().filter_map(mode).collect();
        modes.sort_by(|a, b| b.cmp(a));
        modes.dedup();
        let active = item.get("active").and_then(Value::as_bool).unwrap_or(false);
        let rect = item.get("rect").cloned().unwrap_or(Value::Null);
        list.push(Output {
            name,
            make: text(item, "make"),
            model: text(item, "model"),
            serial: text(item, "serial"),
            active,
            modes,
            current: if active { item.get("current_mode").and_then(mode) } else { None },
            x: number(&rect, "x"),
            y: number(&rect, "y"),
            scale: item.get("scale").and_then(Value::as_f64).filter(|s| *s > 0.0).unwrap_or(1.0),
            transform: Transform::parse(&text(item, "transform")),
            adaptive_sync: text(item, "adaptive_sync_status") == "enabled",
            physical: (number(item, "width"), number(item, "height")),
            focused: item.get("focused").and_then(Value::as_bool).unwrap_or(false),
        })
    }
    list
}

/// A chave de cada saída no perfil: a identidade do monitor. Só quando dois monitores do conjunto têm a mesma
/// (dois iguais sem número de série), o nome da porta desempata: "<id>#HDMI-A-1".
pub fn keys(outputs: &[Output]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| {
            let id = output.identity();
            if outputs.iter().filter(|other| other.identity() == id).count() > 1 {
                format!("{id}#{}", output.name)
            } else {
                id
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!([
            {"name": "eDP-1", "make": "BOE", "model": "0x0BCA", "serial": "Unknown", "active": true, "focused": true,
             "rect": {"x": 0, "y": 0, "width": 1536, "height": 864}, "scale": 1.25, "transform": "normal",
             "adaptive_sync_status": "disabled", "max_render_time": "off",
             "current_mode": {"width": 1920, "height": 1080, "refresh": 60008},
             "modes": [{"width": 1920, "height": 1080, "refresh": 60008}, {"width": 1920, "height": 1080, "refresh": 48006}],
             "width": 344, "height": 194},
            {"name": "HDMI-A-1", "make": "Dell Inc.", "model": "DELL U2720Q", "serial": "ABC123", "active": false,
             "rect": {"x": 0, "y": 0, "width": 0, "height": 0}, "scale": -1.0, "transform": "normal",
             "modes": [{"width": 3840, "height": 2160, "refresh": 60000}, {"width": 2560, "height": 1440, "refresh": 59951},
                       {"width": 3840, "height": 2160, "refresh": 30000}]},
            {"name": "VR-1", "make": "X", "model": "Y", "serial": "Z", "non_desktop": true}
        ])
    }

    #[test]
    fn reads_the_outputs() {
        let list = parse(&sample());
        assert_eq!(list.len(), 2);
        let edp = &list[0];
        assert_eq!(edp.identity(), "BOE 0x0BCA Unknown");
        assert_eq!(edp.description(), "BOE 0x0BCA");
        assert_eq!(edp.current, Some(Mode { width: 1920, height: 1080, refresh: 60008 }));
        assert_eq!(edp.scale, 1.25);
        assert!(is_internal(&edp.name));
        assert!(edp.focused);
        let dell = &list[1];
        assert!(!dell.active);
        assert!(!dell.focused);
        assert_eq!(dell.current, None);
        assert_eq!(dell.scale, 1.0);
        assert_eq!(dell.modes[0], Mode { width: 3840, height: 2160, refresh: 60000 });
        // Desligada: o perfil guarda o maior modo, para religar com ele
        let config = dell.config();
        assert!(!config.enabled);
        assert_eq!(config.mode, Some(dell.modes[0]));
    }

    #[test]
    fn mode_text() {
        let mode = Mode { width: 2560, height: 1440, refresh: 143912 };
        assert_eq!(mode.command(), "2560x1440@143.912Hz");
        assert_eq!(mode.refresh_label(), "143.91 Hz");
        assert_eq!(Mode { width: 1, height: 1, refresh: 60000 }.refresh_label(), "60 Hz");
        assert_eq!(Mode { width: 1, height: 1, refresh: 59500 }.refresh_label(), "59.5 Hz");
        assert_eq!(Mode { width: 1, height: 1, refresh: 60008 }.command(), "1x1@60.008Hz");
        assert_eq!(Mode { width: 800, height: 600, refresh: 0 }.command(), "800x600");
    }

    #[test]
    fn same_monitors_get_the_port() {
        let value = serde_json::json!([
            {"name": "DP-1", "make": "AOC", "model": "24B2W", "serial": "", "active": true},
            {"name": "DP-2", "make": "AOC", "model": "24B2W", "serial": "", "active": true},
            {"name": "eDP-1", "make": "BOE", "model": "X", "serial": "1", "active": true}
        ]);
        let list = parse(&value);
        assert_eq!(keys(&list), vec!["AOC 24B2W Unknown#DP-1", "AOC 24B2W Unknown#DP-2", "BOE X 1"]);
    }
}
