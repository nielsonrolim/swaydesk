//! As teclas como chips: "Mod4+Shift+e" vira [Super] [Shift] [E], e Super+1…9 vira [Super] [1–9].
use std::collections::HashSet;

use crate::parse::{modifier, Bind, Entry, Kind};

/// Marca o texto para o po/update sem traduzir aqui: o xgettext acha as chamadas de tr com texto literal
/// no código Rust. Quem mostra traduz, com o i18n::tr do swaydesk-common; assim os testes daqui não
/// dependem da língua.
const fn tr(text: &'static str) -> &'static str {
    text
}

/// Um chip.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Label {
    /// Mostrado como está (uma letra, um símbolo, F1).
    Text(String),
    /// Uma palavra a traduzir (Enter, Space, Volume up).
    Word(&'static str),
    /// Um bindcode: o nome da tecla depende do layout, e quem mostra o acha (o GDK).
    Code(u32),
    /// Os dedos de um gesto: "4 fingers", no plural da língua.
    Fingers(u32),
}

pub type Chord = Vec<Label>;

const MOD_ORDER: [&str; 8] = ["Super", "Ctrl", "Alt", "Shift", "Mod5", "Mod3", "Mod2", "Caps"];

/// Os chips de um bind.
pub fn chord(bind: &Bind) -> Chord {
    if bind.kind == Kind::Gesture {
        return gesture(&bind.keys);
    }
    let mut mods = Vec::new();
    let mut keys = Vec::new();
    for part in bind.keys.split('+').filter(|p| !p.is_empty()) {
        match modifier(part) {
            Some(m) => mods.push(m),
            None => keys.push(part),
        }
    }
    mods.sort_by_key(|m| MOD_ORDER.iter().position(|o| o == m));
    mods.dedup();
    let mut out: Chord = mods.into_iter().map(mod_label).collect();
    for key in keys {
        out.push(match bind.kind {
            Kind::Code => key.parse().map(Label::Code).unwrap_or_else(|_| key_label(key)),
            _ => key_label(key),
        });
    }
    // Uma tecla de modificador sozinha (Super_L) repete o modificador que ela já é
    out.dedup();
    out
}

fn mod_label(name: &'static str) -> Label {
    match name {
        "Super" => Label::Word(tr("Super")),
        "Ctrl" => Label::Word(tr("Ctrl")),
        "Shift" => Label::Word(tr("Shift")),
        "Alt" => Label::Text("Alt".into()),
        "Mod5" => Label::Text("AltGr".into()),
        "Caps" => Label::Word(tr("Caps Lock")),
        other => Label::Text(other.into()),
    }
}

/// O nome de uma tecla (o keysym do xkb) para mostrar.
pub fn key_label(name: &str) -> Label {
    let word = match name {
        "Return" | "KP_Enter" => tr("Enter"),
        "space" => tr("Space"),
        "BackSpace" => tr("Backspace"),
        "Delete" => tr("Del"),
        "Insert" => tr("Ins"),
        "Home" => tr("Home"),
        "End" => tr("End"),
        "Page_Up" | "Prior" => tr("PgUp"),
        "Page_Down" | "Next" => tr("PgDn"),
        "Print" => tr("Print Screen"),
        "Super_L" | "Super_R" => tr("Super"),
        "Control_L" | "Control_R" => tr("Ctrl"),
        "Shift_L" | "Shift_R" => tr("Shift"),
        "button1" => tr("Left click"),
        "button2" => tr("Middle click"),
        "button3" => tr("Right click"),
        "button4" => tr("Scroll up"),
        "button5" => tr("Scroll down"),
        "button6" => tr("Scroll left"),
        "button7" => tr("Scroll right"),
        "button8" => tr("Back button"),
        "button9" => tr("Forward button"),
        "XF86AudioRaiseVolume" => tr("Volume up"),
        "XF86AudioLowerVolume" => tr("Volume down"),
        "XF86AudioMute" => tr("Mute"),
        "XF86AudioMicMute" => tr("Mute microphone"),
        "XF86AudioPlay" => tr("Play/Pause"),
        "XF86AudioPause" => tr("Pause"),
        "XF86AudioStop" => tr("Stop"),
        "XF86AudioNext" => tr("Next track"),
        "XF86AudioPrev" => tr("Previous track"),
        "XF86MonBrightnessUp" => tr("Brightness up"),
        "XF86MonBrightnessDown" => tr("Brightness down"),
        _ => return Label::Text(symbol(name)),
    };
    Label::Word(word)
}

fn symbol(name: &str) -> String {
    let text = match name {
        "Escape" => "Esc",
        "Tab" => "Tab",
        "Left" => "←",
        "Right" => "→",
        "Up" => "↑",
        "Down" => "↓",
        "Alt_L" | "Alt_R" => "Alt",
        "ISO_Level3_Shift" => "AltGr",
        "bracketleft" => "[",
        "bracketright" => "]",
        "braceleft" => "{",
        "braceright" => "}",
        "parenleft" => "(",
        "parenright" => ")",
        "slash" => "/",
        "backslash" => "\\",
        "question" => "?",
        "minus" => "-",
        "plus" => "+",
        "equal" => "=",
        "comma" => ",",
        "period" => ".",
        "semicolon" => ";",
        "colon" => ":",
        "apostrophe" => "'",
        "quotedbl" => "\"",
        "grave" => "`",
        "asciitilde" => "~",
        "less" => "<",
        "greater" => ">",
        "underscore" => "_",
        "numbersign" => "#",
        "asterisk" => "*",
        "exclam" => "!",
        "at" => "@",
        "dollar" => "$",
        "percent" => "%",
        "ampersand" => "&",
        "bar" => "|",
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                // Uma letra só: maiúscula, como no teclado
                (Some(c), None) => c.to_uppercase().collect(),
                _ => name.strip_prefix("XF86").unwrap_or(name).replace('_', " "),
            };
        }
    };
    text.to_string()
}

/// Os chips de um gesto: "swipe:4:left" vira [Swipe] [4 fingers] [←].
fn gesture(spec: &str) -> Chord {
    let mut out = Vec::new();
    for part in spec.split(':') {
        out.push(match part {
            "swipe" => Label::Word(tr("Swipe")),
            "pinch" => Label::Word(tr("Pinch")),
            "hold" => Label::Word(tr("Hold")),
            "left" => Label::Text("←".into()),
            "right" => Label::Text("→".into()),
            "up" => Label::Text("↑".into()),
            "down" => Label::Text("↓".into()),
            "inward" => Label::Word(tr("Inward")),
            "outward" => Label::Word(tr("Outward")),
            "clockwise" => Label::Word(tr("Clockwise")),
            "counterclockwise" => Label::Word(tr("Counterclockwise")),
            other => match other.parse() {
                Ok(n) => Label::Fingers(n),
                Err(_) => Label::Text(other.to_string()),
            },
        });
    }
    out
}

/// Os chips de uma linha da lista: sem repetir (o press e o --release do Super_L são o mesmo chip), e
/// três ou mais que só mudam no dígito do fim, em sequência, viram um só: Super+1…9 é [Super] [1–9].
pub fn chords(entry: &Entry) -> Vec<Chord> {
    let mut seen = HashSet::new();
    let list: Vec<Chord> = entry.binds.iter().map(chord).filter(|c| seen.insert(c.clone())).collect();
    compress_digits(list)
}

fn compress_digits(list: Vec<Chord>) -> Vec<Chord> {
    if list.len() < 3 {
        return list;
    }
    let prefix = &list[0][..list[0].len().saturating_sub(1)];
    let mut digits = Vec::new();
    for chord in &list {
        let Some((Label::Text(last), head)) = chord.split_last() else { return list };
        let mut chars = last.chars();
        let (Some(d), None) = (chars.next(), chars.next()) else { return list };
        if head != prefix || !d.is_ascii_digit() {
            return list;
        }
        digits.push(d);
    }
    if digits.windows(2).any(|w| w[1] as u32 != w[0] as u32 + 1) {
        return list;
    }
    let mut chord = prefix.to_vec();
    chord.push(Label::Text(format!("{}–{}", digits[0], digits[digits.len() - 1])));
    vec![chord]
}

/// O comando para mostrar: cada caminho vira só o nome do programa ("exec /usr/libexec/swaydesk/sway-shot
/// region" é "exec sway-shot region").
pub fn short_command(command: &str) -> String {
    command
        .split_whitespace()
        .map(|word| {
            let bare = word.trim_matches(|c| c == '"' || c == '\'');
            if (bare.starts_with('/') || bare.starts_with('~')) && bare.len() > 1 {
                bare.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(bare)
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// O atalho de um programa, achado pelo comando: o primeiro bind cujo comando tem as palavras de `needle`
/// em sequência ("sway-capture shot"). None se o usuário tirou o atalho.
pub fn find_shortcut(entries: &[Entry], needle: &str) -> Option<Chord> {
    let wanted: Vec<&str> = needle.split_whitespace().collect();
    if wanted.is_empty() {
        return None;
    }
    entries.iter().filter(|e| e.mode.is_none()).flat_map(|e| &e.binds).find_map(|bind| {
        let words: Vec<String> = short_command(&bind.command).split_whitespace().map(str::to_string).collect();
        words
            .windows(wanted.len())
            .any(|w| w.iter().map(String::as_str).eq(wanted.iter().copied()))
            .then(|| chord(bind))
    })
}
