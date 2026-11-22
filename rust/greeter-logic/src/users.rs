//! Os usuários que aparecem na tela de login: as contas de gente do /etc/passwd (UID entre o UID_MIN e
//! o UID_MAX do /etc/login.defs) com um shell de login.
use std::path::{Path, PathBuf};

use swaydesk_lock_logic::User;

/// O UID_MIN e o UID_MAX do login.defs; sem eles, 1000 e 60000 (os padrões do shadow).
pub fn uid_range(login_defs: &Path) -> (u32, u32) {
    let text = std::fs::read_to_string(login_defs).unwrap_or_default();
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some(key)).then(|| words.next().and_then(|v| v.parse().ok())).flatten()
        })
    };
    (value("UID_MIN").unwrap_or(1000), value("UID_MAX").unwrap_or(60000))
}

/// Um shell que não deixa entrar (nologin, false) tira a conta da lista. Sem shell é o /bin/sh.
fn login_shell(shell: &str) -> bool {
    let name = shell.rsplit('/').next().unwrap_or("");
    !matches!(name, "nologin" | "false")
}

/// As contas, na ordem do arquivo; um nome repetido vale a primeira linha, como no getpwnam.
pub fn users(passwd: &Path, login_defs: &Path) -> Vec<User> {
    let (min, max) = uid_range(login_defs);
    let text = std::fs::read_to_string(passwd).unwrap_or_default();
    let mut found: Vec<User> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 || fields[0].is_empty() || fields[0].starts_with(['+', '-', '#']) {
            continue;
        }
        let Ok(uid) = fields[2].parse::<u32>() else { continue };
        if uid < min || uid > max || !login_shell(fields[6]) || found.iter().any(|u| u.name == fields[0]) {
            continue;
        }
        found.push(User { name: fields[0].to_string(), gecos: fields[4].to_string(), dir: PathBuf::from(fields[5]) });
    }
    found
}

/// Um nome digitado em "Outro usuário": o que o useradd aceita, sem espaço nem barra, para não ir lixo ao
/// greetd (que responde "usuário não existe" do mesmo jeito).
pub fn valid_username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_alphanumeric() || "._-@$".contains(c))
}

/// O texto de um erro do greetd para o log, sem o nome que a pessoa digitou: no campo "Outro usuário" ela pode
/// ter digitado a senha por engano, e uma mensagem como "unable to look up user 'xyz'" (do greetd ou do PAM, a
/// depender da versão) a levaria ao journal. A tela mostra o texto inteiro; só o log troca o nome por <user>.
/// Limites conhecidos: um nome que o greetd ou o PAM devolva transformado (em minúsculas, cortado) escapa da
/// troca, e com os caracteres do valid_username escapar não acontece; um nome curto ("a") troca também as letras
/// iguais do resto da mensagem, o que só deixa o log feio.
pub fn redact_user(text: &str, user: &str) -> String {
    if user.is_empty() {
        return text.to_string();
    }
    text.replace(user, "<user>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typed_name_never_reaches_the_log() {
        assert_eq!(redact_user("unable to look up user 'Senha123x'", "Senha123x"), "unable to look up user '<user>'");
        assert_eq!(redact_user("Senha123x: Senha123x not found", "Senha123x"), "<user>: <user> not found");
        assert_eq!(redact_user("session start failed", "maria"), "session start failed");
        assert_eq!(redact_user("anything", ""), "anything");
    }

    #[test]
    fn people_with_a_login_shell_in_the_uid_range() {
        let dir = std::env::temp_dir().join(format!("swaydesk-greeter-users-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (passwd, defs) = (dir.join("passwd"), dir.join("login.defs"));
        std::fs::write(
            &passwd,
            "root:x:0:0:root:/root:/bin/bash\n\
             _greetd:x:998:998::/var/lib/greetd:/usr/sbin/nologin\n\
             ana:x:1000:1000:Ana Souza,,,:/home/ana:/bin/bash\n\
             bia:x:1001:1001::/home/bia:\n\
             svc:x:1002:1002::/srv:/bin/false\n\
             ana:x:1003:1003:Outra:/home/ana2:/bin/sh\n\
             nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n\
             +nis::::::\n\
             broken:x:abc:1::/:/bin/sh\n",
        )
        .unwrap();
        let names = |list: Vec<User>| list.into_iter().map(|u| u.name).collect::<Vec<_>>();
        assert_eq!(names(users(&passwd, &defs)), ["ana", "bia"]);
        assert_eq!(users(&passwd, &defs)[0].gecos, "Ana Souza,,,");
        std::fs::write(&defs, "# comentário\nUID_MIN\t\t 500\nUID_MAX 1000\n").unwrap();
        assert_eq!(uid_range(&defs), (500, 1000));
        assert_eq!(names(users(&passwd, &defs)), ["ana"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typed_names_are_checked() {
        assert!(valid_username("ana.souza"));
        assert!(valid_username("joão"));
        assert!(!valid_username(""));
        assert!(!valid_username("-ana"));
        assert!(!valid_username("ana souza"));
        assert!(!valid_username("../ana"));
    }
}
