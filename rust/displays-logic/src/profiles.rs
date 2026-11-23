//! Os perfis: cada conjunto de monitores (as identidades de [`crate::outputs::keys`]) lembra o seu layout,
//! em `~/.config/swaydesk/displays.json` (ou na pasta velha, pelo `swaydesk_common::dirs`). O app grava ao "Manter"; o serviço só lê.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::config::{OutputConfig, Transform, SCALE_MAX, SCALE_MIN};
use crate::outputs::Mode;

const VERSION: i64 = 1;

/// O arquivo dos perfis, ao lado do config.toml do desktop
pub fn path() -> PathBuf {
    swaydesk_common::dirs::config_dir().join("displays.json")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// Chave da saída → configuração
    pub outputs: BTreeMap<String, OutputConfig>,
}

impl Profile {
    /// A chave do conjunto: as chaves das saídas, em ordem
    pub fn set(&self) -> Vec<String> {
        self.outputs.keys().cloned().collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profiles {
    pub list: Vec<Profile>,
}

/// O conjunto ordenado, para comparar com o de um perfil
pub fn set_of(keys: &[String]) -> Vec<String> {
    let mut set = keys.to_vec();
    set.sort();
    set.dedup();
    set
}

fn config_to_json(config: &OutputConfig) -> Value {
    let mut map = Map::new();
    map.insert("enabled".into(), json!(config.enabled));
    if let Some(mode) = config.mode {
        map.insert("mode".into(), json!({"width": mode.width, "height": mode.height, "refresh": mode.refresh}));
    }
    map.insert("position".into(), json!([config.x, config.y]));
    map.insert("scale".into(), json!(config.scale));
    map.insert("transform".into(), json!(config.transform.sway()));
    map.insert("adaptive_sync".into(), json!(config.adaptive_sync));
    Value::Object(map)
}

fn int(value: Option<&Value>) -> Option<i32> {
    value.and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok())
}

/// Uma configuração do arquivo. Um valor fora do esperado invalida só ela (e o perfil inteiro, que não
/// pode ser aplicado pela metade).
fn config_from_json(value: &Value) -> Option<OutputConfig> {
    let enabled = value.get("enabled")?.as_bool()?;
    let mode = match value.get("mode") {
        None | Some(Value::Null) => None,
        Some(mode) => {
            let mode = Mode { width: int(mode.get("width"))?, height: int(mode.get("height"))?, refresh: int(mode.get("refresh"))? };
            let valid = mode.width > 0 && mode.height > 0 && mode.width <= 16384 && mode.height <= 16384 && mode.refresh >= 0;
            Some(valid.then_some(mode)?)
        }
    };
    let position = value.get("position")?.as_array()?;
    let (x, y) = (int(position.first())?, int(position.get(1))?);
    let scale = value.get("scale")?.as_f64().filter(|s| (SCALE_MIN..=SCALE_MAX).contains(s))?;
    let transform_text = value.get("transform")?.as_str()?;
    let transform = Transform::parse(transform_text);
    if transform.sway() != transform_text {
        return None;
    }
    let adaptive_sync = value.get("adaptive_sync").and_then(Value::as_bool).unwrap_or(false);
    Some(OutputConfig { enabled, mode, x, y, scale, transform, adaptive_sync })
}

impl Profiles {
    /// Lê os perfis. Arquivo que falta é vazio; ilegível, também, com o motivo no Err para o aviso. Um perfil
    /// com defeito é pulado, os outros valem.
    pub fn load(file: &Path) -> Result<Profiles, String> {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Profiles::default()),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut list = Vec::new();
        for item in value.get("profiles").and_then(Value::as_array).into_iter().flatten() {
            let Some(outputs) = item.get("outputs").and_then(Value::as_object) else { continue };
            let parsed: Option<BTreeMap<String, OutputConfig>> =
                outputs.iter().map(|(key, config)| config_from_json(config).map(|c| (key.clone(), c))).collect();
            if let Some(outputs) = parsed.filter(|o| !o.is_empty()) {
                let profile = Profile { outputs };
                // Repetido: vale o último do arquivo
                list.retain(|p: &Profile| p.set() != profile.set());
                list.push(profile);
            }
        }
        Ok(Profiles { list })
    }

    pub fn to_json(&self) -> Value {
        let profiles: Vec<Value> = self
            .list
            .iter()
            .map(|p| {
                let outputs: Map<String, Value> = p.outputs.iter().map(|(k, c)| (k.clone(), config_to_json(c))).collect();
                json!({"outputs": outputs})
            })
            .collect();
        json!({"version": VERSION, "profiles": profiles})
    }

    /// O perfil do conjunto, se houver
    pub fn find(&self, keys: &[String]) -> Option<&Profile> {
        let set = set_of(keys);
        self.list.iter().find(|p| p.set() == set)
    }

    /// Guarda (ou troca) o perfil do conjunto dele
    pub fn store(&mut self, profile: Profile) {
        let set = profile.set();
        self.list.retain(|p| p.set() != set);
        self.list.push(profile);
    }

    /// Grava num temporário ao lado e troca de nome, com fsync: nunca deixa o arquivo pela metade
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension(format!("json.{}.tmp", std::process::id()));
        let result = (|| {
            let mut out = std::fs::File::create(&tmp)?;
            let bytes = serde_json::to_vec_pretty(&self.to_json()).map_err(std::io::Error::other)?;
            std::io::Write::write_all(&mut out, &bytes)?;
            out.sync_all()?;
            std::fs::rename(&tmp, file)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(x: i32) -> OutputConfig {
        OutputConfig {
            enabled: true,
            mode: Some(Mode { width: 1920, height: 1080, refresh: 60000 }),
            x,
            y: 0,
            scale: 1.25,
            transform: Transform::R90,
            adaptive_sync: false,
        }
    }

    fn profile(entries: &[(&str, OutputConfig)]) -> Profile {
        Profile { outputs: entries.iter().map(|(k, c)| (k.to_string(), *c)).collect() }
    }

    #[test]
    fn stores_finds_and_survives_a_round_trip() {
        let dir = std::env::temp_dir().join(format!("swaydesk-displays-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("sub").join("displays.json");
        assert_eq!(Profiles::load(&file), Ok(Profiles::default()));
        let mut profiles = Profiles::default();
        profiles.store(profile(&[("A a 1", config(0))]));
        profiles.store(profile(&[("A a 1", config(0)), ("B b 2", config(1920))]));
        profiles.store(profile(&[("B b 2", config(5)), ("A a 1", config(7))]));
        assert_eq!(profiles.list.len(), 2);
        profiles.save(&file).unwrap();
        let loaded = Profiles::load(&file).unwrap();
        assert_eq!(loaded, profiles);
        let both = loaded.find(&["B b 2".into(), "A a 1".into()]).unwrap();
        assert_eq!(both.outputs["A a 1"].x, 7);
        assert!(loaded.find(&["C c 3".into()]).is_none());
        assert!(std::fs::read_dir(file.parent().unwrap()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_files_and_bad_profiles() {
        let dir = std::env::temp_dir().join(format!("swaydesk-displays-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("displays.json");
        std::fs::write(&file, "{not json").unwrap();
        assert!(Profiles::load(&file).is_err());
        let good = config_to_json(&config(0));
        let mut bad_scale = good.clone();
        bad_scale["scale"] = json!(40);
        let mut bad_transform = good.clone();
        bad_transform["transform"] = json!("sideways");
        let mut huge = good.clone();
        huge["mode"]["width"] = json!(1_000_000);
        let text = json!({"version": 1, "profiles": [
            {"outputs": {"A": good}},
            {"outputs": {"B": good, "C": bad_scale}},
            {"outputs": {"D": bad_transform}},
            {"outputs": {"E": huge}},
            {"outputs": {}},
            {"nothing": 1}
        ]});
        std::fs::write(&file, text.to_string()).unwrap();
        let loaded = Profiles::load(&file).unwrap();
        assert_eq!(loaded.list.len(), 1);
        assert_eq!(loaded.list[0].set(), vec!["A"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
