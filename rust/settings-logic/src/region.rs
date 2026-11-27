//! O painel Região, sem GTK: os idiomas que a interface tem e o que cada campo grava. Um campo vazio tira a
//! chave (vale o padrão: o idioma do sistema, a cidade do IP, o país do idioma); o resto passa pelas regras do
//! config (layers::check) antes de gravar.

use swaydesk_common::config_edit::{Edit, Value};

use crate::n_;

/// Os idiomas da interface (os catálogos do po/), com o nome no próprio idioma; o primeiro, vazio, é o do sistema.
pub const LANGUAGES: &[(&str, &str)] =
    &[("", n_("Same as the system")), ("en_US", "English"), ("pt_BR", "Português (Brasil)"), ("es_ES", "Español")];

/// A mudança que o texto de um campo ("locale", "weather_location", "holidays") grava.
pub fn edit_for(key: &str, text: &str) -> Result<Edit, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Edit::Remove(key.to_string()));
    }
    // O país em maiúsculas ("br" vira "BR"), e o "none" em minúsculas, como o config pede
    let value = match key {
        "holidays" if text.eq_ignore_ascii_case("none") => "none".to_string(),
        "holidays" => text.to_ascii_uppercase(),
        _ => text.to_string(),
    };
    // O motivo cita o valor como a pessoa o digitou ("Brazil"), não o normalizado ("BRAZIL")
    crate::layers::check(key, &value).map_err(|err| err.replacen(&value, text, 1))?;
    Ok(Edit::Set(key.to_string(), Value::Str(value)))
}

/// O locale que falta gerar para a data seguir o idioma escolhido ("es_ES.UTF-8"), ou nada. Os textos vêm dos
/// catálogos do Swaydesk e mudam sem ele; o relógio e o calendário formatam a data com o locale do sistema, e sem
/// ele ficam na língua do sistema. `generated` diz se um locale existe (o newlocale do libc).
pub fn missing_locale(code: &str, generated: impl Fn(&str) -> bool) -> Option<String> {
    if code.is_empty() {
        return None;
    }
    // O codeset vai antes de um modificador: sr_RS@latin é sr_RS.UTF-8@latin
    let name = match code.split_once('@') {
        Some((base, modifier)) => format!("{base}.UTF-8@{modifier}"),
        None => format!("{code}.UTF-8"),
    };
    (!generated(&name)).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(key: &str, value: &str) -> Result<Edit, String> {
        Ok(Edit::Set(key.to_string(), Value::Str(value.to_string())))
    }

    #[test]
    fn the_languages_are_the_catalogs_and_the_system_one() {
        let codes: Vec<&str> = LANGUAGES.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes, ["", "en_US", "pt_BR", "es_ES"]);
        for code in &codes[1..] {
            assert!(crate::layers::check("locale", code).is_ok(), "{code}");
        }
    }

    #[test]
    fn a_language_without_its_locale_generated_needs_a_note() {
        // A interface tem o catálogo, mas a data (o strftime do relógio) precisa do locale gerado no sistema
        let generated = |name: &str| name == "pt_BR.UTF-8";
        assert_eq!(missing_locale("es_ES", generated), Some("es_ES.UTF-8".to_string()));
        assert_eq!(missing_locale("pt_BR", generated), None);
        // O do sistema (vazio) nunca: vale o LANG, que já é um locale do sistema
        assert_eq!(missing_locale("", generated), None);
        // Com um modificador, o codeset vem antes dele, como o glibc pede
        assert_eq!(missing_locale("sr_RS@latin", generated), Some("sr_RS.UTF-8@latin".to_string()));
    }

    #[test]
    fn an_empty_field_removes_the_key() {
        for key in ["locale", "weather_location", "holidays"] {
            assert_eq!(edit_for(key, "  "), Ok(Edit::Remove(key.to_string())));
        }
    }

    #[test]
    fn a_value_is_trimmed_and_checked() {
        assert_eq!(edit_for("weather_location", "  São Paulo "), set("weather_location", "São Paulo"));
        assert_eq!(edit_for("locale", "pt_BR"), set("locale", "pt_BR"));
        assert!(edit_for("locale", "portuguese!").is_err());
    }

    #[test]
    fn a_holidays_code_is_uppercased_and_none_stays_lowercase() {
        assert_eq!(edit_for("holidays", "br"), set("holidays", "BR"));
        assert_eq!(edit_for("holidays", "None"), set("holidays", "none"));
        assert!(edit_for("holidays", "Brazil").is_err());
    }

    #[test]
    fn a_refused_value_is_named_as_typed() {
        let err = edit_for("holidays", " Brazil ").unwrap_err();
        assert!(err.contains("Brazil") && !err.contains("BRAZIL"), "{err}");
        // Só o valor volta ao que foi digitado: o "BR" do exemplo no motivo fica
        let err = edit_for("holidays", "b").unwrap_err();
        assert!(err.contains(": b") && err.contains("BR"), "{err}");
    }
}
