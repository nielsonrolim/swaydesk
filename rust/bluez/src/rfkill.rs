//! O rfkill do Bluetooth, sem o blueman: o KillSwitch dele sai na base do dconf do Sway (blueman-agent), porque no
//! Debian e no Ubuntu ele pede a senha de administrador a quem não está no grupo sudo ou netdev. Com o adaptador
//! bloqueado por software (a tecla de modo avião, um `rfkill block`), o Powered do BlueZ falha: o popup desbloqueia
//! antes, escrevendo no /dev/rfkill (o systemd dá acesso ao usuário da sessão ativa, a tag uaccess), como o
//! `rfkill unblock bluetooth`. Um bloqueio físico (hard) não sai por software.
use std::io::Write;
use std::path::Path;

pub const SYS_RFKILL: &str = "/sys/class/rfkill";
pub const DEV_RFKILL: &str = "/dev/rfkill";
/// RFKILL_TYPE_BLUETOOTH e RFKILL_OP_CHANGE_ALL do linux/rfkill.h
const TYPE_BLUETOOTH: u8 = 2;
const OP_CHANGE_ALL: u8 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Blocked {
    pub soft: bool,
    pub hard: bool,
}

fn flag(dir: &Path, name: &str) -> bool {
    std::fs::read_to_string(dir.join(name)).is_ok_and(|v| v.trim() == "1")
}

/// Os bloqueios dos rádios Bluetooth em `sys` (/sys/class/rfkill): soft, algum bloqueado por software; hard, todos
/// bloqueados por hardware (com um adaptador interno preso pela tecla e um USB livre, o USB ainda liga).
pub fn bluetooth_blocked(sys: &Path) -> Blocked {
    let (mut soft, mut radios, mut hard) = (false, 0, 0);
    let Ok(entries) = std::fs::read_dir(sys) else { return Blocked::default() };
    for entry in entries.flatten() {
        let dir = entry.path();
        if std::fs::read_to_string(dir.join("type")).is_ok_and(|t| t.trim() == "bluetooth") {
            radios += 1;
            soft |= flag(&dir, "soft");
            hard += usize::from(flag(&dir, "hard"));
        }
    }
    Blocked { soft, hard: radios > 0 && hard == radios }
}

/// O evento que desbloqueia todos os rádios Bluetooth: struct rfkill_event (idx u32, type, op, soft, hard), 8 bytes.
pub fn unblock_event() -> [u8; 8] {
    let mut event = [0u8; 8];
    event[4] = TYPE_BLUETOOTH;
    event[5] = OP_CHANGE_ALL;
    event
}

/// Desbloqueia (por software) todos os rádios Bluetooth pelo `dev` (/dev/rfkill).
pub fn unblock_bluetooth(dev: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new().write(true).open(dev)?.write_all(&unblock_event())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radio(sys: &Path, name: &str, kind: &str, soft: &str, hard: &str) {
        let dir = sys.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, value) in [("type", kind), ("soft", soft), ("hard", hard)] {
            std::fs::write(dir.join(file), format!("{value}\n")).unwrap();
        }
    }

    #[test]
    fn only_bluetooth_radios_count() {
        let sys = std::env::temp_dir().join(format!("swaydesk-rfkill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sys);
        assert_eq!(bluetooth_blocked(&sys), Blocked::default()); // sem /sys/class/rfkill
        radio(&sys, "rfkill0", "wlan", "1", "1");
        radio(&sys, "rfkill1", "bluetooth", "0", "0");
        assert_eq!(bluetooth_blocked(&sys), Blocked { soft: false, hard: false });
        radio(&sys, "rfkill2", "bluetooth", "1", "0");
        assert_eq!(bluetooth_blocked(&sys), Blocked { soft: true, hard: false });
        // Um preso por hardware e outros livres: ainda dá para ligar
        radio(&sys, "rfkill3", "bluetooth", "0", "1");
        assert_eq!(bluetooth_blocked(&sys), Blocked { soft: true, hard: false });
        let _ = std::fs::remove_dir_all(&sys);
        radio(&sys, "rfkill4", "bluetooth", "1", "1");
        assert_eq!(bluetooth_blocked(&sys), Blocked { soft: true, hard: true });
        let _ = std::fs::remove_dir_all(&sys);
    }

    #[test]
    fn the_event_unblocks_every_bluetooth_radio() {
        // O mesmo que o `rfkill unblock bluetooth` escreve (util-linux): idx 0, tipo 2, CHANGE_ALL, soft 0, hard 0
        assert_eq!(unblock_event(), [0, 0, 0, 0, 2, 3, 0, 0]);
        let file = std::env::temp_dir().join(format!("swaydesk-rfkill-dev-{}", std::process::id()));
        std::fs::write(&file, b"").unwrap();
        unblock_bluetooth(&file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), unblock_event());
        let _ = std::fs::remove_file(&file);
        assert!(unblock_bluetooth(Path::new("/nao-existe/rfkill")).is_err());
    }
}
