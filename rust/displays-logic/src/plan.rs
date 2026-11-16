//! O que o serviço de perfis faz quando o conjunto de monitores muda: acha o perfil do conjunto e diz se é
//! preciso aplicar algo. Com a tampa do notebook fechada e outra tela no perfil, a tela interna fica
//! desligada, mesmo que o perfil a tenha ligada (o conjunto é o mesmo com a tampa aberta ou fechada).
use std::path::Path;

use crate::config::OutputConfig;
use crate::outputs::{self, Output};
use crate::profiles::Profiles;

#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Sem perfil para o conjunto: fica o que o Sway escolheu
    NoProfile,
    /// O perfil já está em uso
    Already,
    /// O perfil deixaria todas as telas desligadas: não aplica
    Refused,
    /// Porta → configuração
    Apply(Vec<(String, OutputConfig)>),
}

/// O que o perfil pede para cada saída presente, já com a tampa levada em conta. None: sem perfil.
pub fn wanted(outputs: &[Output], profiles: &Profiles, lid_closed: bool) -> Option<Vec<(String, OutputConfig)>> {
    let keys = outputs::keys(outputs);
    let profile = profiles.find(&keys)?;
    let mut list: Vec<(String, OutputConfig)> = Vec::new();
    for (output, key) in outputs.iter().zip(&keys) {
        let mut config = *profile.outputs.get(key)?;
        // Um modo que a saída não tem mais (outro cabo, outro firmware): o Sway escolhe
        if let Some(mode) = config.mode {
            let known = output.modes.iter().any(|m| {
                m.width == mode.width && m.height == mode.height && (m.refresh - mode.refresh).abs() <= 500
            });
            if !known && !output.modes.is_empty() {
                config.mode = None;
            }
        }
        list.push((output.name.clone(), config));
    }
    let external_on = list.iter().any(|(name, config)| config.enabled && !outputs::is_internal(name));
    if lid_closed && external_on {
        for (name, config) in list.iter_mut() {
            if outputs::is_internal(name) {
                config.enabled = false;
            }
        }
    }
    Some(list)
}

/// O plano para o estado atual
pub fn plan(outputs: &[Output], profiles: &Profiles, lid_closed: bool) -> Plan {
    let Some(list) = wanted(outputs, profiles, lid_closed) else { return Plan::NoProfile };
    if !list.iter().any(|(_, config)| config.enabled) {
        return Plan::Refused;
    }
    let all_match = list.iter().zip(outputs).all(|((_, config), output)| config.matches(&output.config()));
    if all_match {
        Plan::Already
    } else {
        Plan::Apply(list)
    }
}

/// A rede de segurança: com saídas presentes e nenhuma ligada (um disable do config para um conjunto que
/// não existe mais, um perfil de outro tempo), os comandos que ligam todas. None: alguma já está ligada.
pub fn rescue(outputs: &[Output]) -> Option<String> {
    rescue_with(outputs, false)
}

/// O resgate com o primeiro modo de cada saída (o preferido do monitor), para quando o `enable` sozinho
/// não pega: o modo guardado no Sway pode ser um que o backend recusa ao religar.
pub fn rescue_with_mode(outputs: &[Output]) -> Option<String> {
    rescue_with(outputs, true)
}

fn rescue_with(outputs: &[Output], with_mode: bool) -> Option<String> {
    if outputs.is_empty() || outputs.iter().any(|o| o.active) {
        return None;
    }
    let names: Vec<String> = outputs
        .iter()
        .filter(|o| crate::config::safe_name(&o.name))
        .map(|o| match o.modes.first().filter(|_| with_mode) {
            Some(mode) => format!("output {} enable mode {}", o.name, mode.command()),
            None => format!("output {} enable", o.name),
        })
        .collect();
    (!names.is_empty()).then(|| names.join("; "))
}

/// A tampa está fechada? Lê /proc/acpi/button/lid/*/state ("state:      closed"). Sem o arquivo
/// (desktop, VM), aberta.
pub fn lid_closed(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else { return false };
    entries.flatten().any(|entry| {
        std::fs::read_to_string(entry.path().join("state")).map(|text| text.contains("closed")).unwrap_or(false)
    })
}

pub const LID_ROOT: &str = "/proc/acpi/button/lid";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Transform;
    use crate::outputs::{parse, Mode};
    use crate::profiles::Profile;

    fn outputs() -> Vec<Output> {
        parse(&serde_json::json!([
            {"name": "eDP-2", "make": "BOE", "model": "X", "serial": "1", "active": true,
             "rect": {"x": 0, "y": 0}, "scale": 1.25, "transform": "normal",
             "current_mode": {"width": 1920, "height": 1080, "refresh": 60008},
             "modes": [{"width": 1920, "height": 1080, "refresh": 60008}]},
            {"name": "HDMI-A-1", "make": "Dell", "model": "U", "serial": "2", "active": true,
             "rect": {"x": 1536, "y": 0}, "scale": 1.0, "transform": "normal",
             "current_mode": {"width": 2560, "height": 1440, "refresh": 143912},
             "modes": [{"width": 2560, "height": 1440, "refresh": 143912}, {"width": 1920, "height": 1080, "refresh": 60000}]}
        ]))
    }

    fn config(enabled: bool, mode: (i32, i32, i32), x: i32, scale: f64) -> OutputConfig {
        OutputConfig {
            enabled,
            mode: Some(Mode { width: mode.0, height: mode.1, refresh: mode.2 }),
            x,
            y: 0,
            scale,
            transform: Transform::Normal,
            adaptive_sync: false,
        }
    }

    fn profiles(edp: OutputConfig, hdmi: OutputConfig) -> Profiles {
        let mut p = Profiles::default();
        p.store(Profile { outputs: [("BOE X 1".to_string(), edp), ("Dell U 2".to_string(), hdmi)].into_iter().collect() });
        p
    }

    #[test]
    fn no_profile_and_already() {
        assert_eq!(plan(&outputs(), &Profiles::default(), false), Plan::NoProfile);
        // O Sway devolve 143912 para o "143.912Hz" pedido, e 1.25 pode vir 1.2500001: já bate, sem laço
        let same = profiles(config(true, (1920, 1080, 60008), 0, 1.2500001), config(true, (2560, 1440, 143900), 1536, 1.0));
        assert_eq!(plan(&outputs(), &same, false), Plan::Already);
    }

    #[test]
    fn applies_the_difference() {
        let moved = profiles(config(true, (1920, 1080, 60008), 2560, 1.25), config(true, (2560, 1440, 143912), 0, 1.0));
        let Plan::Apply(list) = plan(&outputs(), &moved, false) else { panic!("devia aplicar") };
        assert_eq!(list[0].0, "eDP-2");
        assert_eq!(list[0].1.x, 2560);
        assert_eq!(list[1].1.x, 0);
    }

    #[test]
    fn lid_closed_keeps_the_internal_screen_off() {
        let both_on = profiles(config(true, (1920, 1080, 60008), 0, 1.25), config(true, (2560, 1440, 143912), 1536, 1.0));
        let Plan::Apply(list) = plan(&outputs(), &both_on, true) else { panic!("devia desligar a interna") };
        assert!(!list[0].1.enabled);
        assert!(list[1].1.enabled);
        // A interna sozinha no perfil: a tampa não a desliga (seria tela nenhuma)
        let only_internal = profiles(config(true, (1920, 1080, 60008), 0, 1.25), config(false, (2560, 1440, 143912), 0, 1.0));
        let plan_closed = plan(&outputs(), &only_internal, true);
        let Plan::Apply(list) = plan_closed else { panic!("devia desligar a externa") };
        assert!(list[0].1.enabled);
    }

    #[test]
    fn refuses_all_off_and_drops_unknown_modes() {
        let off = profiles(config(false, (1920, 1080, 60008), 0, 1.0), config(false, (2560, 1440, 143912), 0, 1.0));
        assert_eq!(plan(&outputs(), &off, false), Plan::Refused);
        let odd = profiles(config(true, (1920, 1080, 60008), 0, 1.25), config(true, (3840, 2160, 60000), 1536, 1.0));
        let list = wanted(&outputs(), &odd, false).unwrap();
        assert_eq!(list[1].1.mode, None);
    }

    #[test]
    fn rescues_when_every_screen_is_off() {
        let mut list = outputs();
        assert_eq!(rescue(&list), None);
        for o in list.iter_mut() {
            o.active = false;
        }
        assert_eq!(rescue(&list).as_deref(), Some("output eDP-2 enable; output HDMI-A-1 enable"));
        assert_eq!(
            rescue_with_mode(&list).as_deref(),
            Some("output eDP-2 enable mode 1920x1080@60.008Hz; output HDMI-A-1 enable mode 2560x1440@143.912Hz")
        );
        list[1].modes.clear();
        assert_eq!(rescue_with_mode(&list).as_deref(), Some("output eDP-2 enable mode 1920x1080@60.008Hz; output HDMI-A-1 enable"));
        assert_eq!(rescue(&[]), None);
    }

    #[test]
    fn reads_the_lid() {
        let dir = std::env::temp_dir().join(format!("swaydesk-displays-lid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!lid_closed(&dir));
        std::fs::create_dir_all(dir.join("LID0")).unwrap();
        std::fs::write(dir.join("LID0/state"), "state:      open\n").unwrap();
        assert!(!lid_closed(&dir));
        std::fs::write(dir.join("LID0/state"), "state:      closed\n").unwrap();
        assert!(lid_closed(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
