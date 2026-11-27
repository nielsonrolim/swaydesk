//! Grava chaves no config.toml do usuário sem perder o resto: os comentários, a ordem e as chaves que não
//! são mexidas ficam como estão (o toml_edit). A Central de Configurações grava por aqui: as chaves soltas
//! ("locale") e as das tabelas ("enabled.dock", "enabled.bar.weather"). Um arquivo que não é TOML válido é do
//! usuário: nada muda nele.

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, TableLike};

/// Um valor a gravar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Int(i64),
}

/// Uma mudança: gravar o valor no caminho ("enabled.dock") ou tirar a chave, para valer o padrão (ou a camada do
/// sistema, no [enabled]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Set(String, Value),
    Remove(String),
}

/// O texto com as mudanças. Erro (com a mensagem do parser) num texto inválido, ou num caminho que passa por
/// uma chave que não é tabela.
pub fn apply(text: &str, edits: &[Edit]) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|err: toml_edit::TomlError| err.message().to_string())?;
    for edit in edits {
        match edit {
            Edit::Set(path, value) => set(doc.as_table_mut(), path, value)?,
            Edit::Remove(path) => remove(doc.as_table_mut(), path),
        }
    }
    let out = doc.to_string();
    // O que sai tem de ser lido de volta pelo config
    out.parse::<toml::Table>().map_err(|err| err.message().to_string())?;
    Ok(out)
}

fn split(path: &str) -> Result<(Vec<&str>, &str), String> {
    let mut parts: Vec<&str> = path.split('.').collect();
    let last = parts.pop().filter(|k| !k.is_empty()).ok_or_else(|| format!("invalid key: {path:?}"))?;
    if parts.iter().any(|k| k.is_empty()) {
        return Err(format!("invalid key: {path:?}"));
    }
    Ok((parts, last))
}

fn set(root: &mut Table, path: &str, value: &Value) -> Result<(), String> {
    let (tables, last) = split(path)?;
    let mut table: &mut dyn TableLike = root;
    for name in tables {
        let item = table.entry(name).or_insert_with(|| {
            let mut new = toml_edit::Table::new();
            new.set_implicit(true); // sem chaves próprias, o [enabled] vazio não aparece antes do [enabled.bar]
            Item::Table(new)
        });
        table = item.as_table_like_mut().ok_or_else(|| format!("{name} is not a table in {path}"))?;
    }
    let mut new = match value {
        Value::Str(text) => toml_edit::Value::from(text.as_str()),
        Value::Bool(flag) => toml_edit::Value::from(*flag),
        Value::Int(number) => toml_edit::Value::from(*number),
    };
    match table.get_mut(last) {
        // A linha que já existe: o valor troca, os comentários e os espaços dela ficam
        Some(Item::Value(old)) => {
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        Some(_) => return Err(format!("{path} is a table, not a value")),
        None => {
            table.insert(last, Item::Value(new));
        }
    }
    Ok(())
}

fn remove(root: &mut Table, path: &str) {
    let Ok((tables, last)) = split(path) else { return };
    let mut table: &mut dyn TableLike = root;
    for name in tables {
        match table.get_mut(name).and_then(Item::as_table_like_mut) {
            Some(inner) => table = inner,
            None => return,
        }
    }
    table.remove(last);
}

/// Lê o arquivo (sem ele, vazio), aplica as mudanças e grava de uma vez (um temporário e o rename), no destino de
/// um link: o link continua. Da leitura ao rename, segura um flock no `.<nome>.lock` da mesma pasta: dois
/// gravadores ao mesmo tempo (a Central e o app Tema) não apagam a chave um do outro. Um link quebrado é erro.
pub fn write(file: &Path, edits: &[Edit]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    // O destino de um link (o config.toml pode ser um link para outra pasta): o rename no lugar do link o
    // trocaria por um arquivo
    let target = match std::fs::symlink_metadata(file) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(file).map_err(|err| format!("{}: {err}", file.display()))?,
        _ => file.to_path_buf(),
    };
    let _lock = lock(&target)?;
    let (text, mode) = match std::fs::read_to_string(&target) {
        Ok(text) => (text, std::fs::metadata(&target).map(|m| m.permissions().mode() & 0o7777).unwrap_or(0o644)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (String::new(), 0o644),
        Err(err) => return Err(format!("{}: {err}", target.display())),
    };
    let out = apply(&text, edits)?;
    crate::text::atomic_write(&target, out.as_bytes(), mode).map_err(|err| format!("{}: {err}", target.display()))
}

/// O flock exclusivo do `.<nome>.lock` ao lado do arquivo, solto quando o File fecha.
fn lock(target: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let dir = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = dir.join(format!(".{name}.lock"));
    let error = |err: std::io::Error| format!("{}: {err}", path.display());
    std::fs::create_dir_all(dir).map_err(error)?;
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(&path).map_err(error)?;
    loop {
        // SAFETY: o fd é do File, aberto até o fim desta função ou do chamador
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(file);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(error(err));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(path: &str, value: Value) -> Edit {
        Edit::Set(path.to_string(), value)
    }

    const SAMPLE: &str = "# Minha config\nlocale = \"pt_BR\"  # o idioma\n\n# Os efeitos\n[effects]\nblur = true\n\n[enabled]\n# a dock\ndock = true  # ligada\n\n[enabled.bar]\nweather = false\n";

    #[test]
    fn a_loose_key_changes_in_place_and_keeps_its_comment() {
        let out = apply(SAMPLE, &[set("locale", Value::Str("es".into()))]).unwrap();
        assert_eq!(out, SAMPLE.replace("\"pt_BR\"", "\"es\""));
    }

    #[test]
    fn a_number_is_written_as_a_toml_integer() {
        let out = apply(SAMPLE, &[set("lock_after", Value::Int(5))]).unwrap();
        let table: toml::Table = out.parse().unwrap();
        assert_eq!(table["lock_after"].as_integer(), Some(5), "{out}");
        assert!(out.contains("lock_after = 5\n"), "{out}");
    }

    #[test]
    fn a_new_loose_key_goes_before_the_tables() {
        let out = apply(SAMPLE, &[set("holidays", Value::Str("BR".into()))]).unwrap();
        let table: toml::Table = out.parse().unwrap();
        assert_eq!(table["holidays"].as_str(), Some("BR"));
        assert!(table["effects"].get("holidays").is_none(), "{out}");
        assert!(out.starts_with("# Minha config\nlocale = \"pt_BR\"  # o idioma\n"), "{out}");
        assert!(out.contains("# Os efeitos\n[effects]\nblur = true\n"), "{out}");
    }

    #[test]
    fn a_table_key_changes_in_place_and_keeps_both_comments() {
        let out = apply(SAMPLE, &[set("enabled.dock", Value::Bool(false))]).unwrap();
        assert_eq!(out, SAMPLE.replace("dock = true  # ligada", "dock = false  # ligada"));
    }

    #[test]
    fn missing_tables_are_created() {
        let out = apply("locale = \"es\"\n", &[set("enabled.bar.clock", Value::Bool(false)), set("enabled.osd", Value::Bool(false))]).unwrap();
        let table: toml::Table = out.parse().unwrap();
        assert_eq!(table["enabled"]["osd"].as_bool(), Some(false));
        assert_eq!(table["enabled"]["bar"]["clock"].as_bool(), Some(false));
        assert_eq!(table["locale"].as_str(), Some("es"));
    }

    #[test]
    fn a_new_key_in_an_existing_table_stays_in_it() {
        let out = apply(SAMPLE, &[set("enabled.bar.clock", Value::Bool(false))]).unwrap();
        let table: toml::Table = out.parse().unwrap();
        assert_eq!(table["enabled"]["bar"]["clock"].as_bool(), Some(false));
        assert_eq!(table["enabled"]["bar"]["weather"].as_bool(), Some(false));
        assert!(out.contains("# a dock\ndock = true  # ligada\n"), "{out}");
    }

    #[test]
    fn remove_takes_the_key_out_and_a_missing_key_is_fine() {
        let out = apply(SAMPLE, &[Edit::Remove("enabled.dock".into()), Edit::Remove("enabled.nope".into()), Edit::Remove("nope.x".into())]).unwrap();
        let table: toml::Table = out.parse().unwrap();
        assert!(table["enabled"].get("dock").is_none(), "{out}");
        assert_eq!(table["enabled"]["bar"]["weather"].as_bool(), Some(false));
        assert!(out.contains("[effects]\nblur = true"), "{out}");
    }

    #[test]
    fn an_invalid_file_or_path_is_an_error() {
        assert!(apply("locale = [\n", &[set("locale", Value::Str("es".into()))]).is_err());
        // locale é um texto: não dá para pôr uma chave dentro dele
        assert!(apply(SAMPLE, &[set("locale.x", Value::Bool(true))]).is_err());
        assert!(apply(SAMPLE, &[set("", Value::Bool(true))]).is_err());
    }

    #[test]
    fn two_writers_at_once_keep_both_keys() {
        // A Central e o app Tema (ou dois painéis) gravando juntos: sem a trava, um rename apaga a chave do outro
        let dir = std::env::temp_dir().join(format!("swaydesk-config-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let writers: Vec<_> = (0..4)
            .map(|n| {
                let file = file.clone();
                std::thread::spawn(move || {
                    for i in 0..40 {
                        write(&file, &[set(&format!("enabled.bar.k{n}_{i}"), Value::Bool(false))]).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let table: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert_eq!(table["enabled"]["bar"].as_table().unwrap().len(), 160);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_creates_the_file_and_keeps_a_link() {
        let dir = std::env::temp_dir().join(format!("swaydesk-config-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        write(&file, &[set("enabled.dock", Value::Bool(false))]).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap().parse::<toml::Table>().unwrap()["enabled"]["dock"].as_bool(), Some(false));
        // Pelo link: o destino muda e o link fica
        let link = dir.join("link.toml");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        write(&link, &[set("locale", Value::Str("es".into()))]).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("locale = \"es\"") && text.contains("dock = false"), "{text}");
        // Um arquivo inválido fica como estava
        std::fs::write(&file, "dock = [\n").unwrap();
        assert!(write(&file, &[set("locale", Value::Str("es".into()))]).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "dock = [\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
