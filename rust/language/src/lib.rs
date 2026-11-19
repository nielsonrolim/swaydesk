//! O layout de teclado da Waybar (custom/language), no lugar do sway/language.
//!
//! O sway/language da Waybar conta os layouts num mapa que só é montado na partida, e cada evento de input
//! procura o layout ativo com `map[nome]`, que insere o nome que não acha: o teclado virtual (wtype, teclado
//! na tela, acesso remoto) chega com layout nulo e vira um segundo layout vazio, a casa vazia; depois de um
//! `swaymsg reload`, o "br" volta. O hide-single-layout não tem como se recuperar disso até o próximo login.
//! Aqui, cada evento de input relê o GET_INPUTS inteiro, e só os teclados de verdade contam: com menos de dois
//! layouts, a linha sai com o texto vazio, e a Waybar esconde o módulo (custom, em qualquer versão).
use std::collections::HashMap;

pub mod ipc;
#[cfg(test)]
mod tests;

/// Um teclado do GET_INPUTS: o identificador, os nomes completos dos layouts e o ativo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyboard {
    pub identifier: String,
    pub layouts: Vec<String>,
    pub active: Option<usize>,
}

/// O que o módulo mostra: o nome curto ("br"), o nome completo na dica e a classe do CSS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub short: String,
    pub full: String,
}

/// Um teclado virtual (zwp_virtual_keyboard_v1 do wlroots, como "0:0:wlr_virtual_keyboard_v1") não é um teclado
/// da pessoa: ele não tem layout próprio e só vive enquanto o programa que o criou. O wlroots sempre os cria com
/// vendor:produto 0:0; um teclado USB de verdade com "Virtual" no nome (o de uma VM ou de um KVM) continua contando.
pub fn is_virtual(identifier: &str, name: &str) -> bool {
    identifier.starts_with("0:0:")
        && (identifier.to_ascii_lowercase().contains("virtual") || name.to_ascii_lowercase().contains("virtual"))
}

/// Os teclados de verdade do GET_INPUTS, com pelo menos um layout.
pub fn keyboards(inputs: &serde_json::Value) -> Vec<Keyboard> {
    let Some(list) = inputs.as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for input in list {
        if input.get("type").and_then(|t| t.as_str()) != Some("keyboard") {
            continue;
        }
        let identifier = input.get("identifier").and_then(|v| v.as_str()).unwrap_or("");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if is_virtual(identifier, name) {
            continue;
        }
        let layouts: Vec<String> = input
            .get("xkb_layout_names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|l| l.as_str()).filter(|l| !l.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        if layouts.is_empty() {
            continue;
        }
        let by_index = input
            .get("xkb_active_layout_index")
            .and_then(|v| v.as_u64())
            .map(|i| i as usize)
            .filter(|&i| i < layouts.len());
        let by_name = input
            .get("xkb_active_layout_name")
            .and_then(|v| v.as_str())
            .and_then(|n| layouts.iter().position(|l| l == n));
        out.push(Keyboard { identifier: identifier.to_string(), layouts, active: by_index.or(by_name) });
    }
    out
}

/// O registro do xkb (rules/evdev.xml): o nome completo de cada layout e variante para o nome curto do
/// layout, o mesmo {short} da Waybar ("Portuguese (Brazil, ABNT2)" -> "br").
pub fn registry(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<layout>") {
        let after = &rest[start + "<layout>".len()..];
        let end = after.find("</layout>").unwrap_or(after.len());
        let block = &after[..end];
        rest = &after[end..];
        let (item, variants) = match block.find("<variantList>") {
            Some(i) => (&block[..i], &block[i..]),
            None => (block, ""),
        };
        let Some(short) = tag(item, "name") else { continue };
        if let Some(full) = tag(item, "description") {
            map.entry(full).or_insert_with(|| short.clone());
        }
        let mut v = variants;
        while let Some(i) = v.find("<variant>") {
            let after = &v[i + "<variant>".len()..];
            let end = after.find("</variant>").unwrap_or(after.len());
            if let Some(full) = tag(&after[..end], "description") {
                map.entry(full).or_insert_with(|| short.clone());
            }
            v = &after[end..];
        }
    }
    map
}

/// O texto do primeiro <nome>…</nome>, com as entidades do XML desfeitas.
fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    let value = unescape(text[start..end].trim());
    (!value.is_empty()).then_some(value)
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// O nome curto de um layout: o do registro ou, sem ele, as três primeiras letras do nome, em minúsculas.
fn short_of(full: &str, registry: &HashMap<String, String>) -> String {
    if let Some(short) = registry.get(full) {
        return short.clone();
    }
    full.chars().filter(|c| c.is_alphanumeric()).take(3).collect::<String>().to_lowercase()
}

/// O que mostrar: o teclado com mais layouts (como o sway/language) e, com dois ou mais, o ativo. Dois
/// layouts com o mesmo nome curto ganham um número, na ordem do config ("us1", "us2"), como na Waybar.
pub fn shown(keyboards: &[Keyboard], registry: &HashMap<String, String>) -> Option<Shown> {
    let keyboard = keyboards.iter().fold(None::<&Keyboard>, |best, k| match best {
        Some(b) if b.layouts.len() >= k.layouts.len() => Some(b),
        _ => Some(k),
    })?;
    if keyboard.layouts.len() < 2 {
        return None;
    }
    let shorts: Vec<String> = keyboard.layouts.iter().map(|l| short_of(l, registry)).collect();
    let active = keyboard.active.unwrap_or(0);
    let mut short = shorts[active].clone();
    if shorts.iter().filter(|s| **s == short).count() > 1 {
        let number = shorts[..=active].iter().filter(|s| **s == short).count();
        short = format!("{short}{number}");
    }
    Some(Shown { short, full: keyboard.layouts[active].clone() })
}

/// A linha da Waybar (return-type json). Sem nada a mostrar, o texto vazio esconde o módulo.
pub fn line(shown: Option<&Shown>) -> String {
    let value = match shown {
        Some(s) => serde_json::json!({ "text": s.short, "tooltip": s.full, "class": s.short }),
        None => serde_json::json!({ "text": "", "tooltip": "", "class": "single" }),
    };
    value.to_string()
}
