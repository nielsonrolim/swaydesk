use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::glob::matches;
use crate::parse::{MAX_BYTES, MAX_DEPTH, MAX_LINE, MAX_VALUE};
use crate::keys::key_label;

/// Uma pasta temporária, apagada no fim.
struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "swaydesk-help-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn env(&self) -> Env {
        Env {
            home: self.0.clone(),
            vars: HashMap::from([("XDG_TEST".to_string(), self.0.to_string_lossy().into_owned())]),
        }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parse_text(text: &str) -> Vec<Entry> {
    let tmp = Tmp::new();
    let main = tmp.write(".config/sway/config", text);
    parse(text, &main, &tmp.env())
}

fn w(word: &'static str) -> Label {
    Label::Word(word)
}

fn t(text: &str) -> Label {
    Label::Text(text.into())
}

#[test]
fn sections_descriptions_and_variables() {
    let entries = parse_text(
        "set $mod Mod4\nset $term kitty\n### Apps\n## Open the terminal\nbindsym Ctrl+Alt+t exec $term\n\
         # um comentário comum não corta\n## Close the window\n# nota\nbindsym $mod+q kill\nbindsym $mod+x exec foo\n\nbindsym z exec z\n",
    );
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].section.as_deref(), Some("Apps"));
    assert_eq!(entries[0].description.as_deref(), Some("Open the terminal"));
    assert_eq!(entries[0].binds[0].command, "exec kitty");
    assert_eq!(entries[1].description.as_deref(), Some("Close the window"));
    // Colado ao de cima, entra na mesma linha da lista
    assert_eq!(entries[1].binds.len(), 2);
    assert_eq!(entries[1].binds[1].keys, "Mod4+x");
    assert_eq!(entries[2].section.as_deref(), Some("Apps"));
}

#[test]
fn a_blank_line_or_another_directive_ends_the_description() {
    let entries = parse_text(
        "## One\nbindsym a exec a\n\nbindsym b exec b\n## Two\nbindsym c exec c\nexec x\nbindsym d exec d\n",
    );
    let descs: Vec<_> = entries.iter().map(|e| e.description.as_deref()).collect();
    assert_eq!(descs, [Some("One"), None, Some("Two"), None]);
}

#[test]
fn the_longest_variable_wins() {
    let entries = parse_text("set $m Mod1\nset $mod Mod4\nbindsym $mod+a exec a\nbindsym $m+b exec b\n");
    assert_eq!(entries[0].binds[0].keys, "Mod4+a");
    assert_eq!(entries[1].binds[0].keys, "Mod1+b");
}

#[test]
fn flags_continuations_and_quotes() {
    let entries = parse_text(
        "bindsym --locked --no-repeat XF86AudioMute \\\n  exec \"mute it\"\nbindsym --release Super_L exec s\n",
    );
    assert!(entries[0].binds[0].locked);
    assert_eq!(entries[0].binds[0].command, "exec \"mute it\"");
    assert!(entries[1].binds[0].release);
}

#[test]
fn includes_follow_globs_tilde_env_and_relative_paths() {
    let tmp = Tmp::new();
    tmp.write(".config/sway/config.d/10-a.conf", "### Local\n## From A\nbindsym a exec a\n");
    tmp.write(".config/sway/config.d/50-local.conf", "## From local\nbindsym b exec b\n");
    tmp.write(".config/sway/config.d/.hidden.conf", "bindsym h exec h\n");
    tmp.write(".config/sway/rel/r.conf", "bindsym r exec r\n");
    tmp.write("env/e.conf", "bindsym e exec e\n");
    let text = "### Main\ninclude ~/.config/sway/config.d/*\ninclude rel/*.conf\ninclude $XDG_TEST/env/e.conf\n\
                include /does/not/exist/*\nbindsym z exec z\n";
    let main = tmp.write(".config/sway/config", text);
    let entries = parse(text, &main, &tmp.env());
    let keys: Vec<_> = entries.iter().map(|e| e.binds[0].keys.as_str()).collect();
    assert_eq!(keys, ["a", "b", "r", "e", "z"]);
    // Cada arquivo começa sem a seção de quem o inclui, e ela volta depois
    assert_eq!(entries[0].section.as_deref(), Some("Local"));
    assert_eq!(entries[1].section, None);
    assert_eq!(entries[4].section.as_deref(), Some("Main"));
}

#[test]
fn a_later_bind_with_the_same_keys_wins() {
    let tmp = Tmp::new();
    tmp.write(".config/sway/config.d/50-local.conf", "## Foot instead\nbindsym --no-warn Ctrl+Alt+t exec foot\n");
    let text = "set $mod Mod4\n## Workspaces\nbindsym $mod+1 workspace 1\nbindsym $mod+2 workspace 2\n\
                ## Terminal\nbindsym Ctrl+Alt+t exec kitty\ninclude ~/.config/sway/config.d/*\nbindsym Shift+Mod4+2 exec two\n";
    let main = tmp.write(".config/sway/config", text);
    let entries = parse(text, &main, &tmp.env());
    // O Ctrl+Alt+t do config sumiu (a linha toda), e o Super+2 saiu da linha dos workspaces só se as
    // teclas forem as mesmas: Shift+Super+2 é outro atalho
    let descs: Vec<_> = entries.iter().map(|e| e.description.clone()).collect();
    assert_eq!(descs, [Some("Workspaces".into()), Some("Foot instead".into()), None]);
    assert_eq!(entries[0].binds.len(), 2);

    let entries =
        parse_text("bindsym Mod4+Shift+a exec a\nbindsym shift+mod4+A exec b\nbindsym --release Mod4+Shift+a exec c\n");
    assert_eq!(entries.len(), 2, "o --release é outro bind; maiúsculas e a ordem não");
    assert_eq!(entries[0].binds[0].command, "exec b");
}

#[test]
fn unbind_removes() {
    let entries = parse_text("bindsym Mod4+a exec a\nbindsym Mod4+b exec b\nunbindsym Mod4+a\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].binds[0].keys, "Mod4+b");
}

#[test]
fn modes_and_bind_blocks() {
    let entries = parse_text(
        "mode \"resize\" {\n  ## Grow\n  bindsym l resize grow width 10px\n  bindsym Escape mode default\n}\n\
         bindsym --locked {\n  XF86AudioPlay exec play\n  XF86AudioNext exec next\n}\nbindsym l exec outside\n",
    );
    assert_eq!(entries[0].mode.as_deref(), Some("resize"));
    assert_eq!(entries[0].description.as_deref(), Some("Grow"));
    assert_eq!(entries[0].binds.len(), 2);
    assert!(entries[1].binds[0].locked && entries[2].binds[0].locked);
    assert_eq!(entries[1].mode, None);
    // O l do modo resize não é o l de fora
    assert_eq!(entries.last().unwrap().binds[0].command, "exec outside");
    assert_eq!(entries.len(), 4);
}

#[test]
fn include_cycles_depth_and_size_are_limited() {
    let tmp = Tmp::new();
    let main = tmp.write("c/config", "include ~/c/a\nbindsym m exec m\n");
    tmp.write("c/a", "include ~/c/config\ninclude ~/c/a\ninclude ~/c/b\nbindsym a exec a\n");
    tmp.write("c/b", &format!("bindsym big exec big\n#{}\n", "x".repeat(MAX_BYTES as usize)));
    let text = fs::read_to_string(&main).unwrap();
    let entries = parse(&text, &main, &tmp.env());
    let keys: Vec<_> = entries.iter().map(|e| e.binds[0].keys.as_str()).collect();
    assert_eq!(keys, ["a", "m"]);

    // Uma corrente de includes para no MAX_DEPTH
    for i in 0..20 {
        tmp.write(&format!("d/{i}"), &format!("include ~/d/{}\nbindsym k{i} exec k\n", i + 1));
    }
    let main = tmp.0.join("d/0");
    let text = fs::read_to_string(&main).unwrap();
    assert_eq!(parse(&text, &main, &tmp.env()).len(), MAX_DEPTH + 1);
}

#[test]
fn globs() {
    assert!(matches("*", "50-local.conf"));
    assert!(matches("*.conf", "a.conf"));
    assert!(!matches("*.conf", "a.conf.bak"));
    assert!(matches("[0-9]?-*", "50-local"));
    assert!(!matches("[!0-9]*", "50"));
    assert!(matches("a*b*c", "aXbYbc"));
    assert!(!matches("a?", "a"));
}

#[test]
fn chips() {
    let entries = parse_text(
        "set $mod Mod4\nbindsym Shift+$mod+e exec x\nbindcode $mod+49 exec w\nbindgesture swipe:4:left workspace next\n\
         bindsym --locked XF86AudioRaiseVolume exec up\nbindsym $mod+bracketright layout toggle\n",
    );
    assert_eq!(chord(&entries[0].binds[0]), [w("Super"), w("Shift"), t("E")]);
    assert_eq!(chord(&entries[1].binds[0]), [w("Super"), Label::Code(49)]);
    assert_eq!(chord(&entries[2].binds[0]), [w("Swipe"), Label::Fingers(4), t("←")]);
    assert_eq!(chord(&entries[3].binds[0]), [w("Volume up")]);
    assert_eq!(chord(&entries[4].binds[0]), [w("Super"), t("]")]);
    assert_eq!(key_label("Return"), w("Enter"));
    assert_eq!(key_label("F1"), t("F1"));
    assert_eq!(key_label("Page_Up"), w("PgUp"));
}

#[test]
fn chords_merge_repeats_and_digit_runs() {
    let mut text = String::from("set $mod Mod4\n## Go to workspace\n");
    for i in 1..=9 {
        text.push_str(&format!("bindsym $mod+{i} workspace number {i}\n"));
    }
    text.push_str("## App grid\nbindsym Super_L exec p\nbindsym --release Super_L exec r\n");
    text.push_str("## Focus\nbindsym $mod+1 focus\n");
    let entries = parse_text(&text);
    // O $mod+1 de baixo tira o 1 da linha dos workspaces
    assert_eq!(chords(&entries[0]), [vec![w("Super"), t("2–9")]]);
}

#[test]
fn chords_merge_repeats_and_digit_runs_without_override() {
    let mut text = String::from("set $mod Mod4\n## Go to workspace\n");
    for i in 1..=9 {
        text.push_str(&format!("bindsym $mod+{i} workspace number {i}\n"));
    }
    text.push_str("## App grid\nbindsym Super_L exec p\nbindsym --release Super_L exec r\n");
    let entries = parse_text(&text);
    assert_eq!(chords(&entries[0]), [vec![w("Super"), t("1–9")]]);
    assert_eq!(chords(&entries[1]), [vec![w("Super")]]);
    // Fora de sequência, cada um no seu chip
    let entries = parse_text("## X\nbindsym Mod4+1 a\nbindsym Mod4+3 b\nbindsym Mod4+4 c\n");
    assert_eq!(chords(&entries[0]).len(), 3);
}

#[test]
fn shortcuts_by_command() {
    let entries = parse_text(
        "set $bin ~/.local/bin\nbindsym Mod4+Shift+e exec $bin/sway-capture shot\nbindsym Ctrl+Alt+l exec /usr/libexec/swaydesk/sway-lock\n\
         mode \"x\" {\n bindsym a exec $bin/sway-switcher next\n}\n",
    );
    assert_eq!(find_shortcut(&entries, "sway-capture shot"), Some(vec![w("Super"), w("Shift"), t("E")]));
    assert_eq!(find_shortcut(&entries, "sway-lock"), Some(vec![w("Ctrl"), t("Alt"), t("L")]));
    assert_eq!(find_shortcut(&entries, "sway-capture rec"), None, "sem o atalho, o cartão diz que não há");
    assert_eq!(find_shortcut(&entries, "sway-switcher next"), None, "só o modo padrão");
    assert_eq!(short_command("exec ~/.local/bin/sway-shot region"), "exec sway-shot region");
}

#[test]
fn ipc_messages() {
    let req = ipc::request(9);
    assert_eq!(&req[..6], b"i3-ipc");
    assert_eq!(req.len(), 14);
    let mut header = [0u8; 14];
    header[..6].copy_from_slice(b"i3-ipc");
    header[6..10].copy_from_slice(&5u32.to_ne_bytes());
    header[10..14].copy_from_slice(&9u32.to_ne_bytes());
    assert_eq!(ipc::reply_header(&header).unwrap(), (5, 9));
    header[6..10].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert!(ipc::reply_header(&header).is_err());
    assert_eq!(ipc::config_of(br#"{"config": "bindsym a b"}"#).unwrap(), "bindsym a b");
    assert!(ipc::config_of(b"{}").is_err());
}

#[test]
fn main_config_of_each_session() {
    let env = Env { home: "/home/u".into(), vars: HashMap::new() };
    assert_eq!(main_config_path(&env, false, None), Path::new("/home/u/.config/sway/config"));
    assert_eq!(
        main_config_path(&env, true, Some(Path::new("/usr/bin/swaydesk-help"))),
        Path::new("/usr/share/swaydesk-desktop/sway/config")
    );
    let env = Env { home: "/home/u".into(), vars: HashMap::from([("XDG_CONFIG_HOME".into(), "/cfg".into())]) };
    assert_eq!(main_config_path(&env, false, None), Path::new("/cfg/sway/config"));
}

#[test]
fn a_fifo_in_an_include_is_skipped_without_blocking() {
    let tmp = Tmp::new();
    let fifo = tmp.0.join(".config/sway/config.d/pipe.conf");
    fs::create_dir_all(fifo.parent().unwrap()).unwrap();
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: o caminho é um CString válido
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    tmp.write(".config/sway/config.d/real.conf", "bindsym a exec a\n");
    let text = "include ~/.config/sway/config.d/*\n";
    let main = tmp.write(".config/sway/config", text);
    let entries = parse(text, &main, &tmp.env());
    assert_eq!(entries.len(), 1);
}

#[test]
fn doubling_variables_are_capped() {
    let mut text = String::from("set $v0 xxxxxxxxxxxxxxxx\n");
    for i in 1..60 {
        text.push_str(&format!("set $v{i} $v{}$v{}\n", i - 1, i - 1));
    }
    text.push_str("bindsym a exec $v59\nbindsym b exec $v10\n");
    let entries = parse_text(&text);
    // Do $v13 em diante (128 KiB) o set é ignorado, e nada passa do limite; o $v10 (16 KiB) vale
    assert!(entries.iter().flat_map(|e| &e.binds).all(|b| b.command.len() <= MAX_LINE));
    assert!(entries[0].binds[0].command.len() <= MAX_VALUE + 64);
    assert_eq!(entries[1].binds[0].command.len(), "exec ".len() + (16 << 10));
}
