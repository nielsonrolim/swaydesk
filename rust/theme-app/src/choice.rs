//! O que está escolhido na janela, sem GTK: a família (ou um tema de fora), o estilo e a cor, que é uma das do
//! Papirus ou a do papel de parede (accent_auto). O `Choice` do `sway-theme-app` em Python, e a aplicação pela
//! biblioteca (no lugar do `sway-theme pick|set`).
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

/// O que aplicar: a família no estilo e na cor (`sway-theme pick`), ou um tema com o destaque e as pastas na cor
/// (`sway-theme set`). auto: o accent_auto que fica gravado (o --auto ou o --no-auto), o do interruptor. icons: os
/// ícones escolhidos na lista (o --icons), só quando mudaram; sem eles, a troca de tema mantém os de antes.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Pick { family: String, style: Style, color: String, auto: bool, icons: Option<String> },
    Set { theme: String, accent: String, color: String, auto: bool, icons: Option<String> },
}

pub struct Choice {
    pub families: Vec<Family>,
    pub others: Vec<Theme>,
    pub family: Option<Family>,
    pub other: Option<String>,
    pub style: Style,
    /// A cor do Papirus escolhida à mão (com o auto, a que volta ao desligar o interruptor)
    pub color: String,
    /// A cor do papel de parede (o interruptor ligado)
    pub auto: bool,
    /// A leitura do papel de parede: None não abre, Some(None) sem cor, Some(Some(nome)) a cor do Papirus
    pub wallpaper: Option<Option<&'static str>>,
    /// O modo desligado em cima da cor padrão (papel sem cor ou ilegível): ela fica, até escolher uma amostra
    pub kept_default: bool,
    /// Os ícones escolhidos na lista (a entrada, "Papirus" pelos três) e os em uso quando a janela carregou
    pub icons: String,
    pub loaded_icons: String,
}

/// A cor em uso, como o app a carrega: "" quando o modo está desligado em cima do destaque padrão exato, com as
/// pastas na padrão (folder_color ""), como o accent-auto o deixa com um papel sem cor. Senão a das pastas.
pub fn loaded_color(accent: &str, folder_color: &str, auto: bool, folders: impl FnOnce() -> String) -> String {
    if !auto && accent.eq_ignore_ascii_case(DEFAULT_ACCENT) && folder_color.is_empty() {
        return String::new();
    }
    folders()
}

impl Choice {
    /// A cor começa na primeira das que o app mostra.
    pub fn new(families: Vec<Family>, others: Vec<Theme>, colors: &[String]) -> Choice {
        let color = colors.first().cloned().unwrap_or_else(|| "teal".into());
        Choice {
            families,
            others,
            family: None,
            other: None,
            style: Style::Dark,
            color,
            auto: false,
            wallpaper: None,
            kept_default: false,
            icons: String::new(),
            loaded_icons: String::new(),
        }
    }

    /// A escolha do tema em uso, da cor e do modo; sem família nem tema de fora, a primeira família. A cor ""
    /// com o modo desligado: o destaque padrão exato, que não é uma das amostras (fica até escolher uma).
    pub fn load(&mut self, theme_name: &str, color: &str, auto: bool) {
        self.auto = auto;
        self.kept_default = !auto && color.is_empty();
        let selection = sf::selection(&self.families, &self.others, theme_name, Some(color.to_string()));
        self.family = selection.family;
        self.other = selection.other;
        self.style = selection.style;
        self.color = selection.color.unwrap_or_else(|| color.to_string());
        if self.family.is_none() && self.other.is_none() {
            self.family = self.families.first().cloned();
        }
    }

    /// Os ícones em uso (a entrada da lista): a escolha começa neles.
    pub fn load_icons(&mut self, entry: &str) {
        self.icons = entry.to_string();
        self.loaded_icons = entry.to_string();
    }

    /// Os ícones em uso mudaram por fora (terminal) com uma escolha em andamento: a lista acompanha, a não ser
    /// que a pessoa já tenha mexido nela.
    pub fn follow_icons(&mut self, entry: &str) {
        if self.icons == self.loaded_icons {
            self.icons = entry.to_string();
        }
        self.loaded_icons = entry.to_string();
    }

    /// Os ícones a pedir no apply: só os escolhidos na lista, quando mudaram. Sem mudança, a troca de tema decide
    /// (os da pessoa ficam; os que vieram com o tema acompanham o novo).
    pub fn wanted_icons(&self) -> Option<String> {
        (!self.icons.is_empty() && self.icons != self.loaded_icons).then(|| self.icons.clone())
    }

    /// O interruptor. Ao desligar, a cor à mão começa na que estava na tela: a do papel ou, sem cor do Papirus
    /// nele, a padrão exata (que não é uma das amostras).
    pub fn set_auto(&mut self, active: bool) {
        if self.auto && !active {
            match self.wallpaper.flatten() {
                Some(name) => self.color = name.to_string(),
                None => self.kept_default = true,
            }
        }
        if active {
            self.kept_default = false;
        }
        self.auto = active;
    }

    /// Uma cor escolhida à mão desliga o modo.
    pub fn pick_color(&mut self, name: &str) {
        self.color = name.to_string();
        self.auto = false;
        self.kept_default = false;
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

    /// O nome da cor do Papirus que vale: a escolhida ou, com o auto, a do papel ("" sem cor nele ou sem papel
    /// legível: vale o DEFAULT_ACCENT, e as pastas ficam na mais perto dele).
    pub fn color_name(&self) -> String {
        if self.auto {
            return self.wallpaper.flatten().unwrap_or_default().to_string();
        }
        if self.kept_default {
            return String::new();
        }
        self.color.clone()
    }

    pub fn accent(&self) -> String {
        papirus_color(&self.color_name()).unwrap_or(DEFAULT_ACCENT).to_string()
    }

    /// A cor que o interruptor daria agora, ligado ou não (a amostra ao lado dele).
    pub fn accent_for_auto(&self) -> String {
        self.wallpaper.flatten().and_then(papirus_color).unwrap_or(DEFAULT_ACCENT).to_string()
    }

    pub fn theme(&self) -> Option<String> {
        match &self.family {
            Some(family) => sf::resolve(family, self.style, &self.accent(), &self.color_name()),
            None => self.other.clone(),
        }
    }

    /// Se o tema GTK tem a cor escolhida. Falso quando é a variante mais perto, ou o tema não muda de cor.
    pub fn exact(&self) -> bool {
        let Some(family) = &self.family else { return false };
        let (theme, name) = (self.theme(), self.color_name());
        !name.is_empty() && family.variants.iter().any(|v| Some(&v.theme) == theme.as_ref() && v.color == name)
    }

    /// O mesmo que o accent-auto faria com o interruptor ligado: a família com a cor do papel (pick) ou, sem cor do
    /// Papirus (o DEFAULT_ACCENT), a variante mais perto dele com as pastas na mais perto (set com --folders "").
    pub fn action(&self) -> Option<Action> {
        let (name, auto, icons) = (self.color_name(), self.auto, self.wanted_icons());
        match (&self.family, &self.other) {
            (Some(family), _) if !name.is_empty() => Some(Action::Pick { family: family.name.clone(), style: self.style, color: name, auto, icons }),
            (Some(_), _) => self.theme().map(|theme| Action::Set { theme, accent: self.accent(), color: name, auto, icons }),
            (None, Some(theme)) => Some(Action::Set { theme: theme.clone(), accent: self.accent(), color: name, auto, icons }),
            _ => None,
        }
    }

    /// Para comparar com o que está aplicado: o tema, a cor, o modo e os ícones. Com o auto, a cor não entra (vem
    /// do papel).
    pub fn key(&self) -> (Option<String>, String, bool, String) {
        (self.theme(), if self.auto { String::new() } else { self.color_name() }, self.auto, self.icons.clone())
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
        Action::Pick { family, style, color, auto, icons } => match st::locked(|| st::pick(family, *style, color, Some(*auto), icons.as_deref())).and_then(|r| r) {
            Ok((theme, applied)) => {
                let (code, message) = outcome(&theme, Ok(applied));
                (theme, code, message)
            }
            Err(err) => {
                let (code, message) = outcome("", Err(err));
                (family.clone(), code, message)
            }
        },
        Action::Set { theme, accent, color, auto, icons } => {
            let result = st::locked(|| st::apply(theme, Some(accent), Some(color), None, Some(*auto), icons.as_deref())).and_then(|r| r);
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
        c.load("Mint-Y-Dark-Teal", "teal", false);
        assert_eq!(c.family.as_ref().map(|f| f.name.as_str()), Some("Mint-Y"));
        assert_eq!((c.style, c.color.as_str()), (Style::Dark, "teal"));
        c.load("Arc-Dark", "blue", false);
        assert_eq!((c.family.is_none(), c.other.as_deref(), c.style), (true, Some("Arc-Dark"), Style::Dark));
        assert_eq!(
            c.action(),
            Some(Action::Set { theme: "Arc-Dark".into(), accent: papirus_color("blue").unwrap().into(), color: "blue".into(), auto: false, icons: None })
        );
    }

    #[test]
    fn a_theme_not_installed_keeps_its_name() {
        let mut c = choice();
        c.load("Nada", "teal", false);
        assert_eq!(c.other.as_deref(), Some("Nada"));
    }

    #[test]
    fn the_family_resolves_the_variant_and_says_if_it_is_exact() {
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "teal", false);
        assert_eq!(c.theme().as_deref(), Some("Mint-Y-Dark-Teal"));
        assert!(c.exact());
        c.color = "red".into(); // sem variante vermelha: a mais perto
        assert!(c.theme().is_some());
        assert!(!c.exact());
        assert_eq!(c.key(), (c.theme(), "red".to_string(), false, String::new()));
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "red".into(), auto: false, icons: None }));
    }

    #[test]
    fn auto_takes_the_wallpaper_color_or_the_default() {
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "teal", true);
        c.wallpaper = Some(Some("blue"));
        assert_eq!((c.color_name().as_str(), c.accent()), ("blue", papirus_color("blue").unwrap().to_string()));
        assert_eq!(c.theme().as_deref(), Some("Mint-Y-Dark-Blue"));
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "blue".into(), auto: true, icons: None }));
        // A cor escolhida à mão fica guardada, e a chave não a compara
        assert_eq!(c.color, "teal");
        assert_eq!(c.key(), (Some("Mint-Y-Dark-Blue".into()), String::new(), true, String::new()));
        // Sem cor no papel, ou sem papel: o destaque padrão, na variante mais perto, com as pastas na mais perto
        for reading in [Some(None), None] {
            c.wallpaper = reading;
            assert_eq!((c.color_name().as_str(), c.accent().as_str()), ("", DEFAULT_ACCENT));
            assert!(!c.exact());
            assert_eq!(
                c.action(),
                Some(Action::Set { theme: "Mint-Y-Dark-Teal".into(), accent: DEFAULT_ACCENT.into(), color: String::new(), auto: true, icons: None })
            );
        }
        // Desligado: volta a cor à mão, e o Aplicar desliga o modo
        c.auto = false;
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "teal".into(), auto: false, icons: None }));
        // Um tema de fora das famílias, com a cor do papel
        c.load("Arc-Dark", "teal", true);
        c.wallpaper = Some(Some("red"));
        assert_eq!(
            c.action(),
            Some(Action::Set { theme: "Arc-Dark".into(), accent: papirus_color("red").unwrap().into(), color: "red".into(), auto: true, icons: None })
        );
    }

    #[test]
    fn switching_auto_off_keeps_what_was_on_screen() {
        let mut c = choice();
        // Com a cor do papel: ela vira a cor à mão
        c.load("Mint-Y-Dark-Teal", "teal", true);
        c.wallpaper = Some(Some("blue"));
        c.set_auto(false);
        assert_eq!((c.color.as_str(), c.kept_default), ("blue", false));
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "blue".into(), auto: false, icons: None }));
        // Sem cor no papel, ou ilegível: fica o destaque padrão exato, não a amostra mais perto
        for reading in [Some(None), None] {
            c.load("Mint-Y-Dark-Teal", "teal", true);
            c.wallpaper = reading;
            c.set_auto(false);
            assert!(c.kept_default);
            assert_eq!((c.color_name().as_str(), c.accent().as_str()), ("", DEFAULT_ACCENT));
            assert_eq!(
                c.action(),
                Some(Action::Set { theme: "Mint-Y-Dark-Teal".into(), accent: DEFAULT_ACCENT.into(), color: String::new(), auto: false, icons: None })
            );
            assert_eq!(c.key(), (Some("Mint-Y-Dark-Teal".into()), String::new(), false, String::new()));
        }
        // Até escolher uma amostra, ligar de novo ou recarregar
        c.pick_color("red");
        assert!(!c.kept_default && !c.auto);
        assert_eq!(c.color_name(), "red");
        c.set_auto(true);
        c.set_auto(false);
        assert!(c.kept_default);
        c.set_auto(true);
        assert!(!c.kept_default);
        c.set_auto(false);
        c.load("Mint-Y-Dark-Teal", "teal", false);
        assert!(!c.kept_default);
        assert_eq!(c.color_name(), "teal");
        // Desligar o que já está desligado não muda nada
        c.set_auto(false);
        assert!(!c.kept_default);
    }

    #[test]
    fn the_default_accent_loads_as_no_swatch() {
        let nearest = || "darkcyan".to_string();
        // O padrão exato, com as pastas na padrão e o modo desligado: nenhuma amostra
        assert_eq!(loaded_color(DEFAULT_ACCENT, "", false, nearest), "");
        assert_eq!(loaded_color(&DEFAULT_ACCENT.to_uppercase(), "", false, nearest), "");
        // Com o modo ligado, com pastas numa cor ou com outro destaque: a das pastas, como antes
        assert_eq!(loaded_color(DEFAULT_ACCENT, "", true, nearest), "darkcyan");
        assert_eq!(loaded_color(DEFAULT_ACCENT, "teal", false, || "teal".into()), "teal");
        assert_eq!(loaded_color("#16a085", "", false, || "teal".into()), "teal");
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "", false);
        assert!(c.kept_default && !c.auto);
        assert_eq!((c.color_name().as_str(), c.accent().as_str()), ("", DEFAULT_ACCENT));
        // Mudar só o estilo mantém o padrão
        c.style = Style::Light;
        assert_eq!(
            c.action(),
            Some(Action::Set { theme: "Mint-Y-Teal".into(), accent: DEFAULT_ACCENT.into(), color: String::new(), auto: false, icons: None })
        );
        // A chave bate com a do aplicado (a cor ""), e qualquer amostra é uma mudança, a mais perto também
        assert_eq!(c.key().1, "");
        c.pick_color("darkcyan");
        assert_eq!(c.key().1, "darkcyan");
        assert_eq!(c.accent(), papirus_color("darkcyan").unwrap());
        // Com o modo ligado, "" não vale como padrão guardado
        c.load("Mint-Y-Dark-Teal", "", true);
        assert!(!c.kept_default);
    }

    #[test]
    fn icons_go_only_when_changed_in_the_list() {
        let mut c = choice();
        c.load("Mint-Y-Dark-Teal", "teal", false);
        c.load_icons("Yaru");
        // Sem mudança, nada de --icons: a troca de tema decide (os da pessoa ficam, os do tema acompanham)
        assert_eq!(c.wanted_icons(), None);
        assert_eq!(c.action(), Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "teal".into(), auto: false, icons: None }));
        c.icons = "Papirus".into();
        assert_eq!(c.wanted_icons().as_deref(), Some("Papirus"));
        assert_eq!(
            c.action(),
            Some(Action::Pick { family: "Mint-Y".into(), style: Style::Dark, color: "teal".into(), auto: false, icons: Some("Papirus".into()) })
        );
        // A chave muda com os ícones: o Aplicar liga
        assert_eq!(c.key().3, "Papirus");
        c.icons = "Yaru".into();
        assert_eq!(c.wanted_icons(), None);
        // Sem a lista carregada (""), nunca
        c.load_icons("");
        assert_eq!(c.wanted_icons(), None);
        // Os em uso mudam por fora: a lista acompanha, se não foi mexida; mexida, fica a escolha
        c.load_icons("Papirus");
        c.follow_icons("Yaru");
        assert_eq!((c.icons.as_str(), c.wanted_icons()), ("Yaru", None));
        c.icons = "Adwaita".into();
        c.follow_icons("Papirus");
        assert_eq!((c.icons.as_str(), c.wanted_icons().as_deref()), ("Adwaita", Some("Adwaita")));
    }

    #[test]
    fn picking_keeps_the_style_when_the_family_has_it() {
        let mut c = choice();
        c.load("Arc-Dark", "teal", false);
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
