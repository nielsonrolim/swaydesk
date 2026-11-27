//! O painel Energia e bloqueio, sem GTK: os tempos do sway-idle (lock_after, screen_off_after, suspend_after) e o
//! que cada escolha grava. Os tempos são chaves soltas do config.toml do usuário, em minutos (0 é nunca); quem os
//! aplica é o sway-idle, do swaydesk-caffeine.

use swaydesk_common::config_edit::{Edit, Value};

/// As escolhas das listas, em minutos; 0 é nunca.
pub const CHOICES: &[u32] = &[0, 1, 2, 5, 10, 15, 30, 45, 60];

/// O programa que aplica os tempos e o pacote que o leva.
pub const PROGRAM: &str = "sway-idle";
pub const PACKAGE: &str = "swaydesk-caffeine";

/// As escolhas de uma lista: as CHOICES e, se o config tiver outro valor, ele também, em ordem.
pub fn choices(current: u32) -> Vec<u32> {
    let mut all = CHOICES.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_unstable();
    }
    all
}

/// A mudança que escolher `minutes` numa lista grava.
pub fn edit_for(key: &str, minutes: u32) -> Result<Edit, String> {
    if !swaydesk_common::config::IDLE_KEYS.contains(&key) {
        return Err(format!("not an idle time: {key}"));
    }
    let value = Value::Int(i64::from(minutes));
    crate::layers::check_value(key, value.clone())?;
    Ok(Edit::Set(key.to_string(), value))
}

/// Se a tela apaga antes de bloquear (os dois ligados): o painel avisa, sem trocar a ordem.
pub fn screen_off_before_lock(lock_after: u32, screen_off_after: u32) -> bool {
    lock_after > 0 && screen_off_after > 0 && screen_off_after < lock_after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_start_at_never_and_keep_a_value_of_the_config() {
        assert_eq!(CHOICES, [0, 1, 2, 5, 10, 15, 30, 45, 60]);
        assert_eq!(choices(10), CHOICES);
        assert_eq!(choices(7), [0, 1, 2, 5, 7, 10, 15, 30, 45, 60]);
        assert_eq!(choices(120), [0, 1, 2, 5, 10, 15, 30, 45, 60, 120]);
    }

    #[test]
    fn a_choice_writes_an_integer_checked_by_the_config() {
        assert_eq!(edit_for("lock_after", 5), Ok(Edit::Set("lock_after".into(), Value::Int(5))));
        assert_eq!(edit_for("suspend_after", 0), Ok(Edit::Set("suspend_after".into(), Value::Int(0))));
        assert!(edit_for("screen_off_after", 1441).is_err());
        assert!(edit_for("cursor_size", 5).is_err(), "only the idle keys");
    }

    #[test]
    fn the_hint_is_only_for_a_screen_that_goes_off_before_the_lock() {
        assert!(screen_off_before_lock(10, 5));
        assert!(!screen_off_before_lock(10, 15));
        assert!(!screen_off_before_lock(10, 10));
        assert!(!screen_off_before_lock(0, 5), "never locks: nothing to come before");
        assert!(!screen_off_before_lock(10, 0), "the screen never goes off");
    }
}
