//! O config::load imprime cada aviso uma vez por processo: um componente lê a config várias vezes (a paleta, as
//! traduções, cada recarga), e o mesmo aviso saía de novo no sway.log a cada leitura. Um aviso novo (o arquivo
//! mudou) ainda sai. O teste roda num processo filho (este mesmo binário, com WARN_ONCE_CHILD), para contar o
//! stderr dele inteiro.
use std::process::Command;

const TEST: &str = "load_prints_each_warning_once_per_process";

#[test]
fn load_prints_each_warning_once_per_process() {
    if let Some(file) = std::env::var_os("WARN_ONCE_CHILD") {
        swaydesk_common::config::load();
        swaydesk_common::config::load();
        std::fs::write(file, "accent = \"red\"\n").unwrap();
        swaydesk_common::config::load();
        swaydesk_common::config::load();
        return;
    }
    let home = std::env::temp_dir().join(format!("swaydesk-warn-once-{}", std::process::id()));
    let file = home.join("swaydesk/config.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "cursor_theme = \"Breeze Light\"\n").unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env("WARN_ONCE_CHILD", &file)
        .env("XDG_CONFIG_HOME", &home)
        .env("SWAYDESK_SYSTEM_CONFIG", home.join("none.toml"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&home);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(stderr.matches("config: invalid theme name in cursor_theme: Breeze Light").count(), 1, "{stderr}");
    assert_eq!(stderr.matches("config: invalid accent color: red").count(), 1, "{stderr}");
}
