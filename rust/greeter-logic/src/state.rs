//! A memória da tela de login: o último usuário e a última sessão de cada um, num JSON que só o usuário
//! do greeter grava (`/var/lib/swaydesk-greeter/state.json`, pasta criada pelo tmpfiles.d do pacote).
use std::collections::BTreeMap;
use std::path::Path;

pub const STATE_FILE: &str = "/var/lib/swaydesk-greeter/state.json";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub last_user: String,
    /// Usuário → id da sessão
    pub sessions: BTreeMap<String, String>,
}

impl State {
    /// Lê a memória. Um arquivo que falta, ilegível ou de outro formato é uma memória vazia.
    pub fn load(file: &Path) -> State {
        let Ok(bytes) = std::fs::read(file) else { return State::default() };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return State::default() };
        let last_user = value.get("last_user").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sessions = value
            .get("sessions")
            .and_then(|v| v.as_object())
            .map(|map| map.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default();
        State { last_user, sessions }
    }

    /// Lembra o login que deu certo.
    pub fn remember(&mut self, user: &str, session: &str) {
        self.last_user = user.to_string();
        self.sessions.insert(user.to_string(), session.to_string());
    }

    /// Grava num temporário ao lado e troca de nome: um desligamento no meio não deixa o arquivo cortado. A
    /// pasta precisa ser só do usuário do greeter (0700, pelo tmpfiles.d): o temporário tem nome previsível.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let value = serde_json::json!({"last_user": self.last_user, "sessions": self.sessions});
        let tmp = file.with_extension("json.tmp");
        let mut out = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut out, &serde_json::to_vec_pretty(&value).map_err(std::io::Error::other)?)?;
        // No btrfs e no xfs, um desligamento logo depois do rename sem o fsync pode deixar o arquivo vazio
        out.sync_all()?;
        std::fs::rename(&tmp, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_survives_garbage() {
        let dir = std::env::temp_dir().join(format!("swaydesk-greeter-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        assert_eq!(State::load(&file), State::default());
        let mut state = State::default();
        state.remember("ana", "swaydesk");
        state.remember("bia", "gnome");
        state.save(&file).unwrap();
        let loaded = State::load(&file);
        assert_eq!(loaded.last_user, "bia");
        assert_eq!(loaded.sessions.get("ana").map(String::as_str), Some("swaydesk"));
        assert!(!dir.join("state.json.tmp").exists());
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(State::load(&file), State::default());
        std::fs::write(&file, r#"{"last_user": 3, "sessions": {"ana": 1, "bia": "gnome"}}"#).unwrap();
        let odd = State::load(&file);
        assert_eq!(odd.last_user, "");
        assert_eq!(odd.sessions.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
