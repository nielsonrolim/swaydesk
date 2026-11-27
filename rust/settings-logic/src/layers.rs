//! De onde vem cada valor que os painéis Desktop e Região mostram: do config.toml do usuário, da camada do sistema
//! (/etc/swaydesk/config.toml, só o [enabled] e o [enabled.bar]) ou do padrão. As tabelas cruas, e não o Config:
//! o Config não guarda um `true` do [enabled.bar], e o painel precisa saber se a chave foi dada.

use swaydesk_common::config::DEFAULT_OFF;

/// De onde vem o valor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// O config.toml do usuário tem a chave
    User,
    /// Só a camada do sistema tem a chave (só no [enabled])
    System,
    /// Ninguém deu: o padrão
    Default,
}

/// Um interruptor do [enabled] ou do [enabled.bar].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub on: bool,
    pub source: Source,
}

/// As duas camadas lidas. Um arquivo que falta ou não é TOML válido conta como vazio; o erro de um inválido fica
/// em user_error e system_error, para o painel mostrá-lo no lugar dos interruptores (senão eles mostrariam os
/// padrões, como se o usuário não tivesse dado nada).
#[derive(Clone, Debug, Default)]
pub struct Layers {
    pub user: toml::Table,
    pub system: toml::Table,
    pub user_error: Option<String>,
    pub system_error: Option<String>,
}

impl Layers {
    pub fn parse(user: &str, system: &str) -> Layers {
        let table = |text: &str| match text.parse::<toml::Table>() {
            Ok(table) => (table, None),
            // A mensagem do toml já traz a linha e a coluna ("TOML parse error at line 2, column 1")
            Err(err) => (toml::Table::new(), Some(err.to_string().trim().to_string())),
        };
        let ((user, user_error), (system, system_error)) = (table(user), table(system));
        Layers { user, system, user_error, system_error }
    }

    /// O estado de "dock" ([enabled]) ou de "bar.weather" ([enabled.bar]).
    pub fn enabled(&self, key: &str) -> State {
        if let Some(on) = enabled_value(&self.user, key) {
            return State { on, source: Source::User };
        }
        if let Some(on) = enabled_value(&self.system, key) {
            return State { on, source: Source::System };
        }
        State { on: !DEFAULT_OFF.contains(&key), source: Source::Default }
    }

    /// Uma chave solta ("locale", "weather_location", "holidays", "shortcuts"), só do usuário: a camada do
    /// sistema não tem essas chaves. None sem ela (ou com um valor que não é texto).
    pub fn value(&self, key: &str) -> Option<String> {
        self.user.get(key).and_then(toml::Value::as_str).map(str::to_string)
    }
}

/// O true ou false do [enabled] (ou do [enabled.bar], para "bar.<módulo>") de uma camada; outro valor não conta.
fn enabled_value(table: &toml::Table, key: &str) -> Option<bool> {
    let enabled = table.get("enabled")?.as_table()?;
    match key.strip_prefix("bar.") {
        Some(module) => enabled.get("bar")?.as_table()?.get(module)?.as_bool(),
        None => enabled.get(key)?.as_bool(),
    }
}

/// Se o valor de uma chave solta é aceito pelo config (o parse_config não reclama dele): a mensagem dele se não.
pub fn check(key: &str, value: &str) -> Result<(), String> {
    check_value(key, swaydesk_common::config_edit::Value::Str(value.to_string()))
}

/// O [`check`] de um valor de qualquer tipo (um número, um bool).
pub fn check_value(key: &str, value: swaydesk_common::config_edit::Value) -> Result<(), String> {
    use swaydesk_common::config_edit::{apply, Edit};
    let line = apply("", &[Edit::Set(key.to_string(), value)])?;
    let (_, warnings) = swaydesk_common::config::parse_config(&line);
    match warnings.into_iter().next() {
        None => Ok(()),
        Some(warning) => Err(warning.trim_start_matches("config: ").to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "locale = \"pt_BR\"\nshortcuts = \"i3\"\n[enabled]\ndock = false\n[enabled.bar]\nweather = true\n";
    const SYSTEM: &str = "locale = \"es\"\nweather_location = \"Lisboa\"\n[enabled]\ndock = true\nosd = false\ninput_method = true\n[enabled.bar]\nclock = false\nweather = false\n";

    fn state(on: bool, source: Source) -> State {
        State { on, source }
    }

    #[test]
    fn the_user_key_wins_over_the_system_layer() {
        let layers = Layers::parse(USER, SYSTEM);
        assert_eq!(layers.enabled("dock"), state(false, Source::User));
        assert_eq!(layers.enabled("bar.weather"), state(true, Source::User));
    }

    #[test]
    fn a_key_only_in_the_system_layer_comes_from_it() {
        let layers = Layers::parse(USER, SYSTEM);
        assert_eq!(layers.enabled("osd"), state(false, Source::System));
        assert_eq!(layers.enabled("input_method"), state(true, Source::System));
        assert_eq!(layers.enabled("bar.clock"), state(false, Source::System));
    }

    #[test]
    fn without_any_key_default_off_keys_are_off_and_the_rest_on() {
        let layers = Layers::parse("", "");
        assert_eq!(layers.enabled("dock"), state(true, Source::Default));
        assert_eq!(layers.enabled("bar.media"), state(true, Source::Default));
        assert_eq!(layers.enabled("input_method"), state(false, Source::Default));
    }

    #[test]
    fn an_invalid_value_or_file_counts_as_not_given() {
        let layers = Layers::parse("[enabled]\ndock = \"no\"\n", "osd = [\n");
        assert_eq!(layers.enabled("dock"), state(true, Source::Default));
        assert_eq!(layers.enabled("osd"), state(true, Source::Default));
    }

    #[test]
    fn an_invalid_file_keeps_its_error_for_the_panel() {
        let layers = Layers::parse("locale = \"es\"\n[enabled\ndock = false\n", "osd = [\n");
        let error = layers.user_error.expect("the user's error");
        assert!(error.contains("line 2"), "{error}");
        assert!(layers.system_error.is_some());
        let fine = Layers::parse(USER, SYSTEM);
        assert_eq!((fine.user_error, fine.system_error), (None, None));
    }

    #[test]
    fn loose_keys_come_only_from_the_user() {
        let layers = Layers::parse(USER, SYSTEM);
        assert_eq!(layers.value("locale").as_deref(), Some("pt_BR"));
        assert_eq!(layers.value("shortcuts").as_deref(), Some("i3"));
        // Uma chave solta no /etc é ignorada: a camada do sistema é só o [enabled]
        assert_eq!(layers.value("weather_location"), None);
        assert_eq!(Layers::parse("", SYSTEM).value("locale"), None);
    }

    #[test]
    fn check_uses_the_config_rules() {
        assert!(check("locale", "pt_BR").is_ok());
        assert!(check("locale", "").is_ok());
        assert!(check("locale", "portuguese!").is_err());
        assert!(check("holidays", "BR").is_ok());
        assert!(check("holidays", "none").is_ok());
        assert!(check("holidays", "Brazil").is_err());
        assert!(check("shortcuts", "niri").is_ok());
        assert!(check("shortcuts", "emacs").is_err());
        assert!(check("weather_location", "São Paulo \"centro\"").is_ok());
    }
}
