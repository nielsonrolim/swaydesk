//! Texto para a busca e gravação de arquivos, como em `waybar_clipboard.py`.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// A mensagem de um erro do D-Bus sem o prefixo "GDBus.Error:nome.do.erro: ".
pub fn strip_remote_error(message: &str) -> &str {
    match message.strip_prefix("GDBus.Error:").and_then(|rest| rest.split_once(": ")) {
        Some((_, text)) if !text.is_empty() => text,
        _ => message,
    }
}

/// Minúsculas e sem acentos, para a busca não depender de AÇÃO/ação. Como o `normalize` do Python
/// (casefold, NFKD e sem as marcas combinantes); o casefold vira `to_lowercase`, mais o ß em ss.
pub fn normalize(text: &str) -> String {
    let lower = text.to_lowercase().replace('ß', "ss");
    let plain: String = lower.nfkd().filter(|c| !is_combining_mark(*c)).collect();
    plain.trim().to_string()
}

/// Grava no mesmo diretório e troca de uma vez; não deixa temporário para trás.
pub fn atomic_write(path: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(
            ".{}.{}.{}",
            name.to_string_lossy(),
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    };
    let result = (|| {
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_remote_error_drops_the_prefix() {
        assert_eq!(strip_remote_error("GDBus.Error:org.freedesktop.DBus.Error.ServiceUnknown: sem dono"), "sem dono");
        assert_eq!(strip_remote_error("outro"), "outro");
        assert_eq!(strip_remote_error("GDBus.Error:x.y: "), "GDBus.Error:x.y: ");
    }

    #[test]
    fn normalize_drops_case_and_accents() {
        assert_eq!(normalize("  Configurações AÇÃO "), "configuracoes acao");
        assert_eq!(normalize("Straße"), "strasse");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn atomic_write_replaces_with_mode_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("swaydesk-text-{}", std::process::id()));
        let path = dir.join("sub").join("file");
        atomic_write(&path, b"one", 0o600).unwrap();
        atomic_write(&path, b"two", 0o600).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(dir.join("sub")).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
