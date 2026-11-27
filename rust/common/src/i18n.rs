//! As traduções (domínio sway-desktop), como em `sway_i18n.py`. O texto no código é o inglês; a
//! língua é a do locale do config ou, sem ele, a do ambiente (`config::message_languages`).
//!
//! O catálogo é lido do `.mo` por conta própria, sem o gettext da glibc: este exige que o locale
//! exista no sistema, e o `locale` do config não precisa.
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::OnceLock;

pub use gettext::Catalog;

use crate::config;

pub const DOMAIN: &str = "sway-desktop";

static CATALOG: OnceLock<Catalog> = OnceLock::new();
static XKB_CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Os nomes dos layouts de teclado (o `xkb_active_layout_name` do Sway, em inglês) são traduzidos pelo
/// próprio xkeyboard-config (xkb-data no Debian): `xkeyboard-config-2` nas versões novas, `xkeyboard-config`
/// nas outras. No Ubuntu e no Mint, as traduções vêm nos language-pack-<língua>-base, em
/// /usr/share/locale-langpack (o gettext do Ubuntu procura lá primeiro; o nosso leitor de .mo, não).
const XKB_DOMAINS: [&str; 2] = ["xkeyboard-config-2", "xkeyboard-config"];
const XKB_DIRS: [&str; 3] = ["/usr/local/share/locale", "/usr/share/locale-langpack", "/usr/share/locale"];

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
    load_domains(&[DOMAIN], languages, dirs)
}

fn load_domains(domains: &[&str], languages: &[String], dirs: &[PathBuf]) -> Catalog {
    for language in languages {
        for name in expand(language) {
            for (dir, domain) in dirs.iter().flat_map(|d| domains.iter().map(move |m| (d, m))) {
                let file = dir.join(&name).join("LC_MESSAGES").join(format!("{domain}.mo"));
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

/// O catálogo da primeira destas línguas que tiver um, nestas pastas (os testes de quem usa os textos).
pub fn load_catalog(languages: &[String], dirs: &[PathBuf]) -> Catalog {
    load(languages, dirs)
}

/// Carrega o catálogo da língua da config ou do ambiente. Chamar uma vez, no começo; sem isso, o
/// primeiro texto pedido carrega do mesmo jeito.
pub fn init(config: &config::Config) {
    let env: HashMap<String, String> = std::env::vars().collect();
    let languages = config::message_languages(config, &env);
    let _ = CATALOG.set(load(&languages, &locale_dirs()));
    let _ = XKB_CATALOG.set(load_xkb(&languages, &XKB_DIRS.map(PathBuf::from)));
}

fn load_xkb(languages: &[String], dirs: &[PathBuf]) -> Catalog {
    load_domains(&XKB_DOMAINS, languages, dirs)
}

/// O nome de um layout de teclado na língua da sessão ("Portuguese (Brazil)" → "Portugués (Brasil)").
/// Sem a tradução instalada, o nome em inglês, como veio. Vazio fica vazio (o msgid "" é o cabeçalho).
pub fn layout_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    XKB_CATALOG
        .get_or_init(|| {
            let env: HashMap<String, String> = std::env::vars().collect();
            load_xkb(&config::message_languages(&quiet_config(), &env), &XKB_DIRS.map(PathBuf::from))
        })
        .gettext(name)
        .to_string()
}

/// A config só para a língua, sem imprimir os avisos: quem lê a config de verdade (o config::load do programa) já os
/// imprime, e com o load aqui cada aviso saía duas vezes. O sway_i18n.py faz o mesmo.
fn quiet_config() -> config::Config {
    config::load_config(&config::path()).0
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let env: HashMap<String, String> = std::env::vars().collect();
        load(&config::message_languages(&quiet_config(), &env), &locale_dirs())
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
        assert_eq!(load(&["en_US".into(), "pt_BR".into()], &dirs).gettext("Copy the result"), "Copiar o resultado");
        assert_eq!(load(&["zz_ZZ".into()], &dirs).ngettext("%d app", "%d apps", 2), "%d apps");
    }

    fn repo_locale() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/share/locale")
    }

    // O catálogo do repositório, o mesmo do Python
    #[test]
    fn reads_the_portuguese_catalog_with_plural_and_context() {
        let pt = load(&["pt_BR".into()], &[repo_locale()]);
        assert_eq!(pt.gettext("Copy the result"), "Copiar o resultado");
        assert_eq!(pt.ngettext("%d application", "%d applications", 1), "%d aplicativo");
        assert_eq!(pt.ngettext("%d application", "%d applications", 2), "%d aplicativos");
        assert_eq!(pt.pgettext("decimal separator", "."), ",");
    }

    // O xkeyboard-config de verdade não está no repositório: um .mo mínimo com o nome do domínio novo
    #[test]
    fn layout_names_come_from_the_xkeyboard_config_domain() {
        let dir = std::env::temp_dir().join(format!("i18n-xkb-{}", std::process::id()));
        let messages = dir.join("es/LC_MESSAGES");
        std::fs::create_dir_all(&messages).unwrap();
        std::fs::write(messages.join("xkeyboard-config-2.mo"), mo(&[("Portuguese (Brazil)", "Portugués (Brasil)")])).unwrap();
        let es = load_xkb(&["es_ES.UTF-8".into()], &[dir.clone()]);
        assert_eq!(es.gettext("Portuguese (Brazil)"), "Portugués (Brasil)");
        // Sem tradução do nome, ou sem catálogo na língua: o inglês
        assert_eq!(es.gettext("English (US)"), "English (US)");
        assert_eq!(load_xkb(&["pt_BR".into()], &[dir.clone()]).gettext("Portuguese (Brazil)"), "Portuguese (Brazil)");
        // Ubuntu e Mint: o .mo só numa pasta posterior (/usr/share/locale-langpack), não na primeira
        let empty = std::env::temp_dir().join(format!("i18n-xkb-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(load_xkb(&["es_ES".into()], &[empty.clone(), dir.clone()]).gettext("Portuguese (Brazil)"), "Portugués (Brasil)");
        let at = |d: &str| XKB_DIRS.iter().position(|x| *x == d).unwrap();
        assert!(at("/usr/share/locale-langpack") < at("/usr/share/locale"));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&empty).unwrap();
    }

    /// Um .mo (little-endian) com estes pares, sem hash.
    fn mo(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut pairs: Vec<(&str, &str)> = [("", "Content-Type: text/plain; charset=UTF-8\n")].into_iter().chain(pairs.iter().copied()).collect();
        pairs.sort();
        let n = pairs.len() as u32;
        let (originals, translations) = (28, 28 + 8 * n);
        let mut data_at = 28 + 16 * n;
        let (mut table, mut strings) = (Vec::new(), Vec::new());
        for column in [0, 1] {
            for pair in &pairs {
                let text = if column == 0 { pair.0 } else { pair.1 };
                table.extend((text.len() as u32).to_le_bytes());
                table.extend(data_at.to_le_bytes());
                strings.extend(text.as_bytes());
                strings.push(0);
                data_at += text.len() as u32 + 1;
            }
        }
        let mut out = Vec::new();
        for word in [0x950412de_u32, 0, n, originals, translations, 0, 0] {
            out.extend(word.to_le_bytes());
        }
        out.extend(table);
        out.extend(strings);
        out
    }

    // Um catálogo es só atende todo es_*: es_MX tenta es_MX e depois es
    #[test]
    fn reads_the_spanish_catalog_for_any_country() {
        for language in ["es_ES.UTF-8", "es_MX", "es_AR"] {
            let es = load(&[language.into()], &[repo_locale()]);
            assert_eq!(es.gettext("Copy the result"), "Copiar el resultado");
            assert_eq!(es.ngettext("%d application", "%d applications", 1), "%d aplicación");
            assert_eq!(es.ngettext("%d application", "%d applications", 2), "%d aplicaciones");
            assert_eq!(es.pgettext("decimal separator", "."), ",");
        }
    }
}
