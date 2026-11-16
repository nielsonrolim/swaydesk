//! A configuração de uma saída (ligada, modo, posição, escala, rotação e VRR), o comando do Sway que a
//! aplica e a comparação com tolerância que evita o laço do serviço: o Sway devolve 143912 mHz para um
//! "143.912Hz" pedido, e a escala vem arredondada para múltiplos de 1/120.
use crate::outputs::Mode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transform {
    #[default]
    Normal,
    R90,
    R180,
    R270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    pub const ALL: [Transform; 8] = [
        Transform::Normal,
        Transform::R90,
        Transform::R180,
        Transform::R270,
        Transform::Flipped,
        Transform::Flipped90,
        Transform::Flipped180,
        Transform::Flipped270,
    ];

    /// O nome do Sway. Um valor desconhecido é "normal".
    pub fn parse(text: &str) -> Transform {
        Transform::ALL.into_iter().find(|t| t.sway() == text).unwrap_or_default()
    }

    pub fn sway(self) -> &'static str {
        match self {
            Transform::Normal => "normal",
            Transform::R90 => "90",
            Transform::R180 => "180",
            Transform::R270 => "270",
            Transform::Flipped => "flipped",
            Transform::Flipped90 => "flipped-90",
            Transform::Flipped180 => "flipped-180",
            Transform::Flipped270 => "flipped-270",
        }
    }

    /// Gira 90° ou 270°: a largura e a altura trocam
    pub fn sideways(self) -> bool {
        matches!(self, Transform::R90 | Transform::R270 | Transform::Flipped90 | Transform::Flipped270)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputConfig {
    pub enabled: bool,
    /// Sem modo (saída sem lista de modos): o Sway escolhe
    pub mode: Option<Mode>,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: Transform,
    pub adaptive_sync: bool,
}

/// A menor e a maior escala que o app aceita
pub const SCALE_MIN: f64 = 0.5;
pub const SCALE_MAX: f64 = 4.0;
/// A diferença de taxa (mHz) e de escala que ainda conta como igual
const REFRESH_TOLERANCE: i32 = 500;
const SCALE_TOLERANCE: f64 = 0.01;

/// "1.25", "1.333333", "2": a escala no comando, sem zeros sobrando
pub fn scale_text(scale: f64) -> String {
    let text = format!("{:.6}", scale.clamp(SCALE_MIN, SCALE_MAX));
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl OutputConfig {
    /// O tamanho lógico (o que as janelas veem): o modo dividido pela escala, girado se for o caso
    pub fn logical_size(&self) -> (i32, i32) {
        let Some(mode) = self.mode else { return (0, 0) };
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        let w = (mode.width as f64 / scale).round() as i32;
        let h = (mode.height as f64 / scale).round() as i32;
        if self.transform.sideways() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// O corpo do comando ou do bloco `output`: "enable mode … pos … scale … transform … adaptive_sync …"
    pub fn sway_args(&self) -> String {
        if !self.enabled {
            return "disable".to_string();
        }
        let mut parts = vec!["enable".to_string()];
        if let Some(mode) = self.mode {
            parts.push(format!("mode {}", mode.command()));
        }
        parts.push(format!("pos {} {}", self.x, self.y));
        parts.push(format!("scale {}", scale_text(self.scale)));
        parts.push(format!("transform {}", self.transform.sway()));
        parts.push(format!("adaptive_sync {}", if self.adaptive_sync { "on" } else { "off" }));
        parts.join(" ")
    }

    /// Já está assim? Compara o que o perfil guarda, com tolerância na taxa e na escala. Desligada é igual a
    /// desligada, qualquer que seja o resto. O VRR guardado é o que a saída tinha ao "Manter" (o app não o
    /// muda), então uma saída sem suporte nunca o pede.
    pub fn matches(&self, other: &OutputConfig) -> bool {
        if self.enabled != other.enabled {
            return false;
        }
        if !self.enabled {
            return true;
        }
        let same_mode = match (self.mode, other.mode) {
            (Some(a), Some(b)) => {
                a.width == b.width && a.height == b.height && (a.refresh - b.refresh).abs() <= REFRESH_TOLERANCE
            }
            // Sem modo pedido: qualquer um serve
            (None, _) | (_, None) => true,
        };
        same_mode
            && self.x == other.x
            && self.y == other.y
            && (self.scale - other.scale).abs() <= SCALE_TOLERANCE
            && self.transform == other.transform
            && self.adaptive_sync == other.adaptive_sync
    }
}

/// O nome da porta é seguro no comando: letras ASCII, números, hífen, sublinhado e ponto. Outro nome não vai
/// para o Sway.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Os comandos que aplicam um conjunto, por porta: as ligadas antes, as desligadas depois (o Sway recusa
/// desligar a última ligada). Um só `swaymsg` com tudo junto, separado por ";".
pub fn commands(configs: &[(String, OutputConfig)]) -> String {
    let mut ordered: Vec<&(String, OutputConfig)> = configs.iter().filter(|(name, _)| safe_name(name)).collect();
    ordered.sort_by_key(|(_, config)| !config.enabled);
    ordered.iter().map(|(name, config)| format!("output {name} {}", config.sway_args())).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(width: i32, height: i32, refresh: i32) -> OutputConfig {
        OutputConfig {
            enabled: true,
            mode: Some(Mode { width, height, refresh }),
            x: 0,
            y: 0,
            scale: 1.0,
            transform: Transform::Normal,
            adaptive_sync: false,
        }
    }

    #[test]
    fn transform_names() {
        for t in Transform::ALL {
            assert_eq!(Transform::parse(t.sway()), t);
        }
        assert_eq!(Transform::parse("bogus"), Transform::Normal);
        assert!(Transform::R90.sideways() && Transform::Flipped270.sideways() && !Transform::Flipped180.sideways());
    }

    #[test]
    fn logical_size_uses_scale_and_rotation() {
        let mut config = on(2560, 1440, 60000);
        config.scale = 1.25;
        assert_eq!(config.logical_size(), (2048, 1152));
        config.transform = Transform::R270;
        assert_eq!(config.logical_size(), (1152, 2048));
        config.mode = None;
        assert_eq!(config.logical_size(), (0, 0));
    }

    #[test]
    fn command_text() {
        let mut config = on(2560, 1440, 143912);
        config.x = 1920;
        config.scale = 1.3333333;
        config.transform = Transform::Flipped90;
        assert_eq!(
            config.sway_args(),
            "enable mode 2560x1440@143.912Hz pos 1920 0 scale 1.333333 transform flipped-90 adaptive_sync off"
        );
        config.enabled = false;
        assert_eq!(config.sway_args(), "disable");
        assert_eq!(scale_text(2.0), "2");
        assert_eq!(scale_text(9.0), "4");
    }

    #[test]
    fn commands_enable_first_and_skip_odd_names() {
        let mut off = on(1920, 1080, 60000);
        off.enabled = false;
        let text = commands(&[
            ("eDP-2".into(), off),
            ("HDMI-A-1".into(), on(1920, 1080, 60000)),
            ("bad; exec rm".into(), on(1, 1, 1)),
        ]);
        assert_eq!(
            text,
            "output HDMI-A-1 enable mode 1920x1080@60.000Hz pos 0 0 scale 1 transform normal adaptive_sync off; output eDP-2 disable"
        );
    }

    #[test]
    fn matches_with_tolerance() {
        let wanted = on(2560, 1440, 143912);
        let mut got = on(2560, 1440, 143856);
        got.scale = 1.0000001;
        assert!(wanted.matches(&got));
        got.x = 1;
        assert!(!wanted.matches(&got));
        got.x = 0;
        got.transform = Transform::R90;
        assert!(!wanted.matches(&got));
        got.transform = Transform::Normal;
        got.adaptive_sync = true;
        assert!(!wanted.matches(&got));
        got.adaptive_sync = false;
        got.mode = Some(Mode { width: 2560, height: 1440, refresh: 59951 });
        assert!(!wanted.matches(&got));
        // Desligadas: o resto não importa
        let mut a = wanted;
        a.enabled = false;
        let mut b = got;
        b.enabled = false;
        assert!(a.matches(&b));
        assert!(!a.matches(&wanted));
    }
}
