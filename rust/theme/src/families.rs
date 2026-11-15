//! Os temas GTK agrupados por família (Mint-Y, Mint-L, Adwaita, Yaru...), com o estilo claro ou escuro e a
//! variante de cor. O porte do `sway_theme_families.py`: o app e o `sway-theme pick` usam o mesmo.
use std::sync::LazyLock;

use regex::Regex;
use swaydesk_common::i18n::tr;

use crate::papirus;
use crate::themes::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Light,
    Dark,
}

impl Style {
    pub fn parse(text: &str) -> Option<Style> {
        match text {
            "light" => Some(Style::Light),
            "dark" => Some(Style::Dark),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::Light => "light",
            Style::Dark => "dark",
        }
    }

    pub fn label(self) -> String {
        tr(self.name())
    }
}

/// O Yaru e o Adwaita guardam o CSS dentro de um gresource: as cores ficam aqui.
const FIXED_ACCENTS: &[(&str, &str)] = &[
    ("Adwaita", "#3584e4"), ("Adwaita-dark", "#3584e4"), ("Yaru", "#e95420"), ("Yaru-bark", "#787859"),
    ("Yaru-sage", "#657b69"), ("Yaru-olive", "#4b8501"), ("Yaru-viridian", "#03875b"), ("Yaru-prussiangreen", "#308280"),
    ("Yaru-blue", "#0073e5"), ("Yaru-purple", "#7764d8"), ("Yaru-magenta", "#b34cb3"), ("Yaru-red", "#da3450"),
];
/// A cor das variantes sem cor no nome: a do tema puro
const BASE_COLORS: &[(&str, &str)] =
    &[("Mint-Y", "green"), ("Mint-L", "green"), ("Mint-L Darker", "green"), ("Mint-X", "green"), ("Yaru", "orange")];

/// (nome, padrão). O regex crate não tem lookahead: as exclusões do Python ((?!Dark), (?!dark$)) ficam em
/// `accepts`, conferidas depois.
static FAMILIES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("Mint-Y", r"^Mint-Y(?:-(?P<dark>Dark))?(?:-(?P<color>[A-Z][a-z]+))?$"),
        ("Mint-L", r"^Mint-L(?:-(?P<dark>Dark))?(?:-(?P<color>[A-Z][a-z]+))?$"),
        ("Mint-L Darker", r"^Mint-L-Darker(?:-(?P<color>[A-Z][a-z]+))?$"),
        ("Mint-X", r"^Mint-X(?:-(?P<color>[A-Z][a-z]+))?$"),
        ("Adwaita", r"^Adwaita(?:-(?P<dark>dark))?$"),
        ("Yaru", r"^Yaru(?:-(?P<color>[a-z]+))?(?:-(?P<dark>dark))?$"),
    ]
    .into_iter()
    .map(|(name, pattern)| (name, Regex::new(pattern).unwrap()))
    .collect()
});

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub theme: String,
    pub style: Style,
    /// A cor do tema; None se não há como saber
    pub accent: Option<String>,
    /// O nome da cor, em minúsculas, como "teal"; vazio se o tema não tem
    pub color: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub name: String,
    pub variants: Vec<Variant>,
}

impl Family {
    pub fn has_style(&self, style: Style) -> bool {
        self.variants.iter().any(|v| v.style == style)
    }

    /// As cores das variantes, uma vez cada, para a lista mostrar.
    pub fn accents(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for accent in self.variants.iter().filter_map(|v| v.accent.clone()) {
            if !seen.contains(&accent) {
                seen.push(accent);
            }
        }
        seen
    }
}

fn fixed(name: &str) -> Option<&'static str> {
    FIXED_ACCENTS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

pub fn accent_of(theme: &Theme) -> Option<String> {
    theme.accent.clone().or_else(|| fixed(&theme.name).or_else(|| fixed(theme.name.strip_suffix("-dark").unwrap_or(&theme.name))).map(str::to_string))
}

/// As exclusões do Python sem lookahead. O (?!Dark) do Mint-Y e do Mint-L recusa uma cor que começa com
/// Dark (o Mint-L-Darker é de outra família); sem outra divisão possível, o nome não é desta família.
/// O (?!dark$) do Yaru faz o "Yaru-dark" ser o estilo, e não a cor. Devolve (escuro, cor).
fn accepts(family: &str, dark: Option<&str>, color: Option<&str>) -> Option<(bool, Option<String>)> {
    match (family, dark, color) {
        ("Mint-Y" | "Mint-L", _, Some(c)) if c.starts_with("Dark") => None,
        ("Yaru", None, Some("dark")) => Some((true, None)),
        _ => Some((dark.is_some(), color.map(str::to_string))),
    }
}

/// (família, estilo, nome da cor) do tema, ou None se ele não é de nenhuma.
pub fn parse(name: &str) -> Option<(&'static str, Style, String)> {
    for (family, pattern) in FAMILIES.iter() {
        let Some(caps) = pattern.captures(name) else { continue };
        let Some((dark, color)) = accepts(family, caps.name("dark").map(|m| m.as_str()), caps.name("color").map(|m| m.as_str())) else {
            continue;
        };
        let base = BASE_COLORS.iter().find(|(n, _)| n == family).map(|(_, c)| *c).unwrap_or("");
        let color = color.unwrap_or_else(|| base.to_string()).to_lowercase();
        return Some((family, if dark { Style::Dark } else { Style::Light }, color));
    }
    None
}

/// (família, estilo) do tema.
pub fn family_of(name: &str) -> Option<(&'static str, Style)> {
    parse(name).map(|(f, s, _)| (f, s))
}

/// (famílias instaladas, na ordem de FAMILIES; temas de fora delas).
pub fn group(themes: &[Theme]) -> (Vec<Family>, Vec<Theme>) {
    let mut families: Vec<Family> = FAMILIES.iter().map(|(n, _)| Family { name: n.to_string(), variants: Vec::new() }).collect();
    let mut others = Vec::new();
    for theme in themes {
        match parse(&theme.name) {
            Some((family, style, color)) => {
                if let Some(f) = families.iter_mut().find(|f| f.name == family) {
                    f.variants.push(Variant { theme: theme.name.clone(), style, accent: accent_of(theme), color });
                }
            }
            None => others.push(theme.clone()),
        }
    }
    (families.into_iter().filter(|f| !f.variants.is_empty()).collect(), others)
}

/// O tema GTK da família, no estilo: o da cor com o mesmo nome ou, sem ele, o da cor mais perto do hex.
pub fn resolve(family: &Family, style: Style, color: &str, name: &str) -> Option<String> {
    let variants: Vec<&Variant> = family.variants.iter().filter(|v| v.style == style).collect();
    let first = variants.first()?;
    if !name.is_empty() {
        if let Some(same) = variants.iter().find(|v| v.color == name) {
            return Some(same.theme.clone());
        }
    }
    let with_color: Vec<&&Variant> = variants.iter().filter(|v| v.accent.is_some()).collect();
    if with_color.is_empty() {
        return Some(first.theme.clone());
    }
    // No empate, o nome mais curto; depois, o primeiro (o min do Python)
    let mut best: Option<(&Variant, f64)> = None;
    for v in with_color {
        let d = papirus::distance(color, v.accent.as_deref().unwrap_or_default());
        let better = match best {
            None => true,
            Some((b, bd)) => d < bd || (d == bd && v.theme.len() < b.theme.len()),
        };
        if better {
            best = Some((v, d));
        }
    }
    best.map(|(v, _)| v.theme.clone())
}

pub fn style_of(theme: &Theme) -> Style {
    if theme.dark { Style::Dark } else { Style::Light }
}

/// Os ícones de painel do Papirus são claros, como a Waybar: o Papirus-Light é para painel claro.
pub fn icons_for_style(style: Style) -> &'static str {
    if style == Style::Dark { "Papirus-Dark" } else { "Papirus" }
}

/// O que está em uso: a família (ou o nome do tema de fora), o estilo e a cor, que vem de quem chama.
pub struct Selection {
    pub family: Option<Family>,
    pub other: Option<String>,
    pub style: Style,
    pub color: Option<String>,
}

pub fn selection(families: &[Family], others: &[Theme], theme_name: &str, color: Option<String>) -> Selection {
    let parsed = family_of(theme_name);
    if let Some((name, style)) = parsed {
        if let Some(family) = families.iter().find(|f| f.name == name) {
            return Selection { family: Some(family.clone()), other: None, style, color };
        }
    }
    let style = match others.iter().find(|t| t.name == theme_name) {
        Some(other) => style_of(other),
        None => parsed.map_or(Style::Light, |(_, s)| s),
    };
    Selection { family: None, other: Some(theme_name.to_string()), style, color }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_like_python() {
        let cases: &[(&str, Option<(&str, Style, &str)>)] = &[
            ("Mint-Y", Some(("Mint-Y", Style::Light, "green"))),
            ("Mint-Y-Dark", Some(("Mint-Y", Style::Dark, "green"))),
            ("Mint-Y-Dark-Teal", Some(("Mint-Y", Style::Dark, "teal"))),
            ("Mint-Y-Aqua", Some(("Mint-Y", Style::Light, "aqua"))),
            ("Mint-L-Darker-Aqua", Some(("Mint-L Darker", Style::Light, "aqua"))),
            ("Mint-L-Darker", Some(("Mint-L Darker", Style::Light, "green"))),
            ("Adwaita-dark", Some(("Adwaita", Style::Dark, ""))),
            ("Yaru", Some(("Yaru", Style::Light, "orange"))),
            ("Yaru-dark", Some(("Yaru", Style::Dark, "orange"))),
            ("Yaru-blue-dark", Some(("Yaru", Style::Dark, "blue"))),
            ("Yaru-blue", Some(("Yaru", Style::Light, "blue"))),
            ("Arc-Dark", None),
            ("Mint-Y-Darker", None),
            ("Mint-L-Darker-Darkblue", Some(("Mint-L Darker", Style::Light, "darkblue"))),
        ];
        for (name, expected) in cases {
            let got = parse(name).map(|(f, s, c)| (f, s, c));
            assert_eq!(got, expected.map(|(f, s, c)| (f, s, c.to_string())), "{name}");
        }
    }
}
