//! O que está escolhido na janela, sem GTK: a família (ou um tema de fora), o estilo e a cor. O `Choice`
//! do `sway-theme-app` em Python, e a aplicação pela biblioteca (no lugar do `sway-theme pick|set`).
use swaydesk_common::config::{papirus_color, DEFAULT_ACCENT};
use swaydesk_common::i18n::tr;
use swaydesk_theme::families::{self as sf, Family, Style};
use swaydesk_theme::themes::{self as st, Applied, Theme};
use swaydesk_theme::{fill_one, Error};

/// Todas as palavras, em qualquer ordem, em algum dos nomes: "aqua dark" acha o Mint-Y.
pub fn matches(names: &[String], text: &str) -> bool {
    let text = text.to_lowercase();
    let words: Vec<&str> = text.split_whitespace().collect();
    names.iter().any(|name| {
        let name = name.to_lowercase();
        words.iter().all(|word| name.contains(word))
    })
}

/// O que aplicar: a família no estilo e na cor (`sway-theme pick`), ou um tema de fora com o destaque
/// e as pastas na cor (`sway-theme set`).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Pick { family: String, style: Style, color: String },
    Set { theme: String, accent: String, color: String },
}

pub struct Choice {
    pub families: Vec<Family>,
    pub others: Vec<Theme>,
    pub family: Option<Family>,
    pub other: Option<String>,
    pub style: Style,
    pub color: String,
}

impl Choice {
    /// A cor começa na primeira das que o app mostra.
    pub fn new(families: Vec<Family>, others: Vec<Theme>, colors: &[String]) -> Choice {
        let color = colors.first().cloned().unwrap_or_else(|| "teal".into());
        Choice { families, others, family: None, other: None, style: Style::Dark, color }
    }

    /// A escolha do tema em uso e da cor; sem família nem tema de fora, a primeira família.
    pub fn load(&mut self, theme_name: &str, color: &str) {
        let selection = sf::selection(&self.families, &self.others, theme_name, Some(color.to_string()));
        self.family = selection.family;
        self.other = selection.other;
        self.style = selection.style;
        self.color = selection.color.unwrap_or_else(|| color.to_string());
        if self.family.is_none() && self.other.is_none() {
            self.family = self.families.first().cloned();
        }
    }

    /// Fica no estilo de antes, se a família o tiver; senão, o primeiro em ordem alfabética (o escuro).
    pub fn pick_family(&mut self, family: &Family) {
        self.family = Some(family.clone());
        self.other = None;
        if !family.has_style(self.style) {
            self.style = if family.has_style(Style::Dark) { Style::Dark } else { Style::Light };
        }
    }

    pub fn pick_other(&mut self, theme: &Theme) {
        self.family = None;
        self.other = Some(theme.name.clone());
        self.style = sf::style_of(theme);
    }

    /// Os estilos que dá para escolher: os da família; num tema de fora, nenhum.
    pub fn has_style(&self, style: Style) -> bool {
        self.family.as_ref().is_some_and(|f| f.has_style(style))
    }

    pub fn accent(&self) -> String {
        papirus_color(&self.color).unwrap_or(DEFAULT_ACCENT).to_string()
    }

    pub fn theme(&self) -> Option<String> {
        match &self.family {
            Some(family) => sf::resolve(family, self.style, &self.accent(), &self.color),
            None => self.other.clone(),
        }
    }

    /// Se o tema GTK tem a cor escolhida. Falso quando é a variante mais perto, ou o tema não muda de cor.
    pub fn exact(&self) -> bool {
        let Some(family) = &self.family else { return false };
        let theme = self.theme();
        family.variants.iter().any(|v| Some(&v.theme) == theme.as_ref() && v.color == self.color)
    }

    pub fn action(&self) -> Option<Action> {
        match (&self.family, &self.other) {
            (Some(family), _) => Some(Action::Pick { family: family.name.clone(), style: self.style, color: self.color.clone() }),
            (None, Some(theme)) => Some(Action::Set { theme: theme.clone(), accent: self.accent(), color: self.color.clone() }),
            _ => None,
        }
    }

    /// Para comparar com o que está aplicado.
    pub fn key(&self) -> (Option<String>, String) {
        (self.theme(), self.color.clone())
    }
}

/// O resultado, como o código de saída do `sway-theme`: 0 aplicado; 1 aplicado, mas algum programa não
/// acompanhou; 2 nada mudou. E a mensagem: a última linha que o comando mostraria.
pub fn outcome(theme: &str, result: Result<Applied, Error>) -> (i32, String) {
    match result {
        Ok(applied) if !applied.failures.is_empty() => (1, fill_one(&tr("Could not update: %s"), &applied.failures.join(", "))),
        Ok(applied) => (0, applied.notes.last().cloned().unwrap_or_else(|| fill_one(&tr("Theme: %s"), theme))),
        Err(err) => (2, err.to_string().lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string()),
    }
}

/// Aplica na trava do sway-theme (fora da linha principal: reinicia a barra e grava o dconf). Devolve o
/// tema aplicado, o código e a mensagem.
pub fn apply(action: &Action) -> (String, i32, String) {
    match action {
        Action::Pick { family, style, color } => match st::locked(|| st::pick(family, *style, color)).and_then(|r| r) {
            Ok((theme, applied)) => {
                let (code, message) = outcome(&theme, Ok(applied));
                (theme, code, message)
            }
            Err(err) => {
                let (code, message) = outcome("", Err(err));
                (family.clone(), code, message)
            }
        },
        Action::Set { theme, accent, color } => {
            let result = st::locked(|| st::apply(theme, Some(accent), Some(color), None)).and_then(|r| r);
            let (code, message) = outcome(theme, result);
            (theme.clone(), code, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swaydesk_theme::families::Variant;

    fn theme(name: &str, dark: bool, accent: Option<&str>) -> Theme {
        Theme { name: name.into(), accent: accent.map(str::to_string), icons: None, dark }
    }

    fn mint() -> Family {
        let v = |theme: &str, style, accent: &str, color: &str| Variant {
            theme: theme.into(),
            style,
            accent: Some(accent.into()),
            color: color.into(),
        };
        Family {
            name: "Mint-Y".into(),
            variants: vec![
                v("Mint-Y-Dark-Teal", Style::Dark, "#199ca8", "teal"),
                v("Mint-Y-Dark-Blue", Style::Dark, "#0c75de", "blue"),
                v("Mint-Y-Teal", Style::Light, "#199ca8", "teal"),
            ],
        }
    }

    fn choice() -> Choice {
        Choice::new(vec![mint()], vec![theme("Arc-Dark", true, None)], &["teal".into(), "blue".into(), "red".into()])
    }

    #[test]
    fn search_takes_words_in_any_order() {
        let names = vec!["Mint-Y".to_string(), "Mint-Y-Dark-Aqua".to_string()];
        assert!(matches(&names, "aqua dark"));
        assert!(matches(&names, ""));
        assert!(matches(&names, "MINT"));
        assert!(!matches(&names, "aqua light"));
    }

    #[test]
    fn load_finds_the_family_or_the_other_theme() {
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "teal");
        assert_eq!(c.family.as_ref().map(|f| f.name.as_str()), Some("Mint-Y"));
        assert_eq!((c.style, c.color.as_str()), (Style::Dark, "teal"));
        c.load("Arc-Dark", "blue");
        assert_eq!((c.family.is_none(), c.other.as_deref(), c.style), (true, Some("Arc-Dark"), Style::Dark));
        assert_eq!(c.action(), Some(Action::Set { theme: "Arc-Dark".into(), accent: papirus_color("blue").unwrap().into(), color: "blue".into() }));
    }

    #[test]
    fn a_theme_not_installed_keeps_its_name() {
        let mut c = choice();
        c.load("Nada", "teal");
        assert_eq!(c.other.as_deref(), Some("Nada"));
    }

    #[test]
    fn the_family_resolves_the_variant_and_says_if_it_is_exact() {
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "teal");
        assert_eq!(c.theme().as_deref(), Some("Mint-Y-Dark-Teal"));
        assert!(c.exact());
        c.color = "red".into(); // sem variante vermelha: a mais perto
        assert!(c.theme().is_some());
        assert!(!c.exact());
        assert_eq!(c.key(), (c.theme(), "red".to_string()));
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "red".into() }));
    }

    #[test]
    fn picking_keeps_the_style_when_the_family_has_it() {
        let mut c = choice();
        c.load("Arc-Dark", "teal");
        c.style = Style::Light;
        c.pick_family(&mint());
        assert_eq!(c.style, Style::Light);
        assert!(c.has_style(Style::Light) && c.has_style(Style::Dark));
        let only_light = Family { name: "X".into(), variants: vec![mint().variants[2].clone()] };
        c.style = Style::Dark;
        c.pick_family(&only_light);
        assert_eq!(c.style, Style::Light);
        c.pick_other(&theme("Arc-Dark", true, None));
        assert!(!c.has_style(Style::Light) && !c.has_style(Style::Dark));
        assert_eq!(c.style, Style::Dark);
    }

    #[test]
    fn outcome_is_the_exit_code_and_the_last_line() {
        let ok = |notes: &[&str], failures: &[&str]| Applied {
            notes: notes.iter().map(|s| s.to_string()).collect(),
            failures: failures.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(outcome("T", Ok(ok(&[], &[]))), (0, "Theme: T".to_string()));
        assert_eq!(outcome("T", Ok(ok(&["a", "b"], &[]))), (0, "b".to_string()));
        assert_eq!(outcome("T", Ok(ok(&["a"], &["x", "y"]))), (1, "Could not update: x, y".to_string()));
        assert_eq!(outcome("T", Err(Error::Value("theme not installed: T".into()))), (2, "theme not installed: T".to_string()));
    }
}
