//! As traduções (domínio sway-desktop), como em `sway_i18n.py`. O texto no código é o inglês; a
//! língua é a do locale do config ou, sem ele, a do ambiente (`config::message_languages`).
//!
//! O catálogo é lido do `.mo` por conta própria, sem o gettext da glibc: este exige que o locale
//! exista no sistema, e o `locale` do config não precisa.
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::OnceLock;

use gettext::Catalog;

use crate::config;

pub const DOMAIN: &str = "sway-desktop";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Onde procurar `<língua>/LC_MESSAGES/sway-desktop.mo`, em ordem: a pasta do install.sh, a que
/// fica ao lado do programa (`bin/../share/locale`, como no Python) e as do sistema.
fn locale_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("SWAY_I18N_LOCALEDIR").filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(prefix) = std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()).and_then(|exe| {
        exe.parent().and_then(|bin| bin.parent()).map(|p| p.to_path_buf())
    }) {
        dirs.push(prefix.join("share/locale"));
    }
    dirs.push(config::data_home().join("locale"));
    dirs.push(PathBuf::from("/usr/local/share/locale"));
    dirs.push(PathBuf::from("/usr/share/locale"));
    dirs
}

/// "pt_BR" também tenta "pt", como o gettext.
fn expand(language: &str) -> Vec<String> {
    let base = language.split('.').next().unwrap_or("").split('@').next().unwrap_or("");
    let mut names = vec![base.to_string()];
    if let Some((lang, _)) = base.split_once('_') {
        names.push(lang.to_string());
    }
    names.retain(|n| !n.is_empty());
    names
}

fn load(languages: &[String], dirs: &[PathBuf]) -> Catalog {
    for language in languages {
        for name in expand(language) {
            for dir in dirs {
                let file = dir.join(&name).join("LC_MESSAGES").join(format!("{DOMAIN}.mo"));
                if let Ok(handle) = File::open(&file) {
                    match Catalog::parse(handle) {
                        Ok(catalog) => return catalog,
                        Err(err) => eprintln!("i18n: could not read {}: {err}", file.display()),
                    }
                }
            }
        }
    }
    Catalog::empty()
}

/// Carrega o catálogo da língua da config ou do ambiente. Chamar uma vez, no começo; sem isso, o
/// primeiro texto pedido carrega do mesmo jeito.
pub fn init(config: &config::Config) {
    let env: HashMap<String, String> = std::env::vars().collect();
    let _ = CATALOG.set(load(&config::message_languages(config, &env), &locale_dirs()));
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let env: HashMap<String, String> = std::env::vars().collect();
        load(&config::message_languages(&config::load(), &env), &locale_dirs())
    })
}

pub fn tr(msgid: &str) -> String {
    catalog().gettext(msgid).to_string()
}

/// O plural, com o `%d` trocado pelo número.
pub fn ntr(singular: &str, plural: &str, n: u64) -> String {
    catalog().ngettext(singular, plural, n).replace("%d", &n.to_string())
}

pub fn ptr(context: &str, msgid: &str) -> String {
    catalog().pgettext(context, msgid).to_string()
}

/// O número com o separador decimal da língua: 12.5 em inglês, 12,5 em português (o `decimal` do
/// `sway_i18n.py`).
pub fn decimal(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', &ptr("decimal separator", "."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_territory_and_drops_encoding() {
        assert_eq!(expand("pt_BR.UTF-8@euro"), ["pt_BR", "pt"]);
        assert_eq!(expand("de"), ["de"]);
    }

    #[test]
    fn english_or_missing_catalog_keeps_the_source_text() {
        let dirs = [repo_locale()];
        assert_eq!(load(&["en_US".into()], &dirs).gettext("All"), "All");
        // Como o gettext do Python: a primeira língua com catálogo
        assert_eq!(load(&["en_US".into(), "pt_BR".into()], &dirs).gettext("Type to search"), "Digite para buscar");
        assert_eq!(load(&["zz_ZZ".into()], &dirs).ngettext("%d app", "%d apps", 2), "%d apps");
    }

    fn repo_locale() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/share/locale")
    }

    // O catálogo do repositório, o mesmo do Python
    #[test]
    fn reads_the_portuguese_catalog_with_plural_and_context() {
        let pt = load(&["pt_BR".into()], &[repo_locale()]);
        assert_eq!(pt.gettext("Type to search"), "Digite para buscar");
        assert_eq!(pt.ngettext("%d application", "%d applications", 1), "%d aplicativo");
        assert_eq!(pt.ngettext("%d application", "%d applications", 2), "%d aplicativos");
        assert_eq!(pt.pgettext("decimal separator", "."), ",");
    }
}
