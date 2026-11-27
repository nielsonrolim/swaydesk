//! Os painéis Desktop e Região, sem GTK: os interruptores do [enabled] que o painel mostra, os nomes dos
//! módulos do [enabled.bar], o programa que cada interruptor liga (para dizer quando não está instalado) e como
//! cada mudança passa a valer.

use crate::n_;

/// O programa por trás de um interruptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    /// Um componente: o lançador dos dotfiles e o binário do pacote (o paths::component)
    Component(&'static str, &'static str),
    /// Um script auxiliar (o paths::find_helper)
    Helper(&'static str),
}

/// Um interruptor do [enabled] no painel Desktop.
#[derive(Debug)]
pub struct SwitchInfo {
    pub key: &'static str,
    /// O nome, em inglês (o msgid do po/)
    pub label: &'static str,
    pub program: Program,
    /// O pacote que traz o programa, para o "Não instalado"
    pub package: &'static str,
}

/// Os do painel, na ordem dele (a linha Desktop do documento da 0.2.0).
pub const SWITCHES: &[SwitchInfo] = &[
    switch("dock", n_("Dock"), Program::Component("sway-dock", "swaydesk-dock"), "swaydesk-dock"),
    switch("app_grid", n_("App grid"), Program::Component("sway-drawer", "swaydesk-drawer"), "swaydesk-drawer"),
    switch("switcher", n_("Window switcher (Alt+Tab)"), Program::Component("sway-switcher", "swaydesk-switcher"), "swaydesk-switcher"),
    switch("osd", n_("Volume and brightness indicator"), Program::Component("sway-osd-bar", "swaydesk-osd-bar"), "swaydesk-osd"),
    switch("nightlight", n_("Night light"), Program::Helper("sway-nightlight"), "swaydesk-brightness"),
    switch("bing_wallpaper", n_("Wallpaper of the day"), Program::Helper("bing-wallpaper"), "swaydesk-desktop"),
    switch("update_notifier", n_("Update notifications"), Program::Helper("sway-update-notifier"), "swaydesk-desktop"),
];

const fn switch(key: &'static str, label: &'static str, program: Program, package: &'static str) -> SwitchInfo {
    SwitchInfo { key, label, program, package }
}

/// Os nomes dos módulos do [enabled.bar], na ordem do BAR_MODULES.
pub const BAR_LABELS: &[(&str, &str)] = &[
    ("weather", n_("Weather")),
    ("clock", n_("Clock")),
    ("media", n_("Media")),
    ("recording", n_("Screen recording")),
    ("notifications", n_("Notifications")),
    ("clipboard", n_("Clipboard")),
    ("language", n_("Keyboard layout")),
    ("system", n_("System")),
    ("brightness", n_("Brightness")),
    ("caffeine", n_("Caffeine")),
    ("bluetooth", n_("Bluetooth")),
    ("network", n_("Network")),
    ("volume", n_("Volume")),
    ("battery", n_("Battery")),
    ("power", n_("Power menu")),
    ("updates", n_("Updates")),
];

/// Como uma mudança passa a valer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    /// Na hora (ou no próximo minuto): quem usa relê o arquivo (o ícone do tempo, o calendário)
    Now,
    /// O sway-waybar restart (os módulos da barra)
    Waybar,
    /// O desktop-config keys e o swaymsg reload (o perfil de atalhos)
    Keys,
    /// No próximo login (os componentes da sessão, o idioma)
    NextLogin,
}

/// Como a chave ("dock", "bar.weather", "shortcuts", "locale"…) passa a valer.
pub fn apply_for(key: &str) -> Apply {
    match key {
        _ if key.starts_with("bar.") => Apply::Waybar,
        "shortcuts" => Apply::Keys,
        // O ícone do tempo é um processo novo a cada minuto (interval 60 no config da Waybar), que relê a cidade;
        // o calendário lê o país a cada vez que abre
        "weather_location" | "holidays" => Apply::Now,
        // Os do [enabled] (o sway-if-enabled só olha no login) e o locale (o ambiente da sessão)
        _ => Apply::NextLogin,
    }
}

/// Uma aplicação por vez: dois cliques seguidos na barra não podem rodar dois `sway-waybar restart` juntos (os dois
/// matariam a Waybar e subiriam um laço cada, e ficariam duas). Um pedido durante a execução só marca que falta mais
/// uma, que roda (uma vez só, com o arquivo já gravado) quando a atual termina.
#[derive(Debug, Default)]
pub struct Serial {
    running: bool,
    pending: bool,
}

impl Serial {
    /// Um pedido: diz se começa agora (senão fica para depois da atual).
    pub fn request(&mut self) -> bool {
        if self.running {
            self.pending = true;
            return false;
        }
        self.running = true;
        true
    }
    /// A atual terminou: diz se roda mais uma.
    pub fn finished(&mut self) -> bool {
        self.running = std::mem::take(&mut self.pending);
        self.running
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_apply_at_a_time_and_one_more_for_the_requests_meanwhile() {
        let mut serial = super::Serial::default();
        assert!(serial.request(), "the first one starts");
        assert!(!serial.request(), "a second one waits");
        assert!(!serial.request(), "a third one joins the second");
        assert!(serial.finished(), "one more after the first");
        assert!(!serial.finished(), "and nothing after it");
        assert!(serial.request(), "idle again: starts at once");
        assert!(!serial.finished());
    }

    use super::*;
    use swaydesk_common::config::{BAR_MODULES, ENABLED_KEYS};

    #[test]
    fn the_switches_are_the_documents_list_of_enabled_keys() {
        let keys: Vec<&str> = SWITCHES.iter().map(|s| s.key).collect();
        assert_eq!(keys, ["dock", "app_grid", "switcher", "osd", "nightlight", "bing_wallpaper", "update_notifier"]);
        assert!(keys.iter().all(|k| ENABLED_KEYS.contains(k)));
        assert!(SWITCHES.iter().all(|s| !s.label.is_empty()));
    }

    #[test]
    fn each_switch_names_its_program() {
        let program = |key| SWITCHES.iter().find(|s| s.key == key).unwrap().program;
        assert_eq!(program("dock"), Program::Component("sway-dock", "swaydesk-dock"));
        assert_eq!(program("app_grid"), Program::Component("sway-drawer", "swaydesk-drawer"));
        assert_eq!(program("switcher"), Program::Component("sway-switcher", "swaydesk-switcher"));
        assert_eq!(program("osd"), Program::Component("sway-osd-bar", "swaydesk-osd-bar"));
        assert_eq!(program("nightlight"), Program::Helper("sway-nightlight"));
        assert_eq!(program("bing_wallpaper"), Program::Helper("bing-wallpaper"));
        assert_eq!(program("update_notifier"), Program::Helper("sway-update-notifier"));
    }

    #[test]
    fn each_switch_names_the_package_that_brings_its_program() {
        let package = |key| SWITCHES.iter().find(|s| s.key == key).unwrap().package;
        assert_eq!(package("dock"), "swaydesk-dock");
        assert_eq!(package("app_grid"), "swaydesk-drawer");
        assert_eq!(package("switcher"), "swaydesk-switcher");
        assert_eq!(package("osd"), "swaydesk-osd");
        assert_eq!(package("nightlight"), "swaydesk-brightness");
        assert_eq!(package("bing_wallpaper"), "swaydesk-desktop");
        assert_eq!(package("update_notifier"), "swaydesk-desktop");
    }

    #[test]
    fn every_bar_module_has_a_label_in_its_order() {
        let keys: Vec<&str> = BAR_LABELS.iter().map(|(k, _)| *k).collect();
        let modules: Vec<&str> = BAR_MODULES.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, modules);
        assert!(BAR_LABELS.iter().all(|(_, label)| !label.is_empty()));
    }

    #[test]
    fn each_change_applies_the_way_its_reader_needs() {
        assert_eq!(apply_for("bar.weather"), Apply::Waybar);
        // O ícone do tempo é um processo novo a cada minuto (interval 60), que relê o config: sem restart (#425)
        assert_eq!(apply_for("weather_location"), Apply::Now);
        assert_eq!(apply_for("shortcuts"), Apply::Keys);
        assert_eq!(apply_for("holidays"), Apply::Now);
        assert_eq!(apply_for("dock"), Apply::NextLogin);
        assert_eq!(apply_for("update_notifier"), Apply::NextLogin);
        assert_eq!(apply_for("locale"), Apply::NextLogin);
    }
}
