//! O painel Teclado, sem GTK: os layouts que se podem escolher (o registro do xkeyboard-config, o
//! rules/evdev.xml) e a tecla que troca de layout (uma opção grp: do xkb_options).

use crate::input::Layout;

/// O registro do xkeyboard-config: o xkeyboard-config-2 das versões novas, senão o de sempre.
pub const REGISTRIES: [&str; 2] =
    ["/usr/share/xkeyboard-config-2/rules/evdev.xml", "/usr/share/X11/xkb/rules/evdev.xml"];

/// Um layout (ou uma variante dele) que se pode escolher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub layout: Layout,
    /// O nome em inglês do registro ("Portuguese (Brazil, ABNT2)"), traduzido pelo catálogo do xkeyboard-config.
    pub description: String,
}

/// Os layouts e variantes do registro, na ordem do nome; os nomes que o 80-input.conf não aceitaria ficam de fora.
pub fn catalog(xml: &str) -> Vec<Choice> {
    let list = xml.split("</layoutList>").next().unwrap_or("");
    let mut out = Vec::new();
    for block in list.split("<layout>").skip(1) {
        let block = block.split("</layout>").next().unwrap_or("");
        let (item, variants) = block.split_once("<variantList>").unwrap_or((block, ""));
        let (Some(layout), Some(description)) = (tag(item, "name"), tag(item, "description")) else { continue };
        if !crate::input::xkb_name(&layout) {
            continue;
        }
        out.push(Choice { layout: Layout { layout: layout.clone(), variant: String::new() }, description });
        for variant in variants.split("<variant>").skip(1) {
            if let (Some(name), Some(description)) = (tag(variant, "name"), tag(variant, "description")) {
                if crate::input::xkb_name(&name) {
                    out.push(Choice { layout: Layout { layout: layout.clone(), variant: name }, description });
                }
            }
        }
    }
    out.sort_by(|a, b| a.description.cmp(&b.description));
    out
}

/// O texto do primeiro <nome>…</nome>, com as entidades do XML desfeitas.
fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))? + start;
    let value = text[start..end]
        .trim()
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    (!value.is_empty()).then_some(value)
}

/// A tecla que troca de layout, além do atalho do perfil (Super+Espaço).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// Só o atalho do perfil.
    Shortcut,
    AltShift,
    CtrlShift,
    CapsLock,
    /// Outra opção grp: que o painel não oferece: mostrada, e mantida enquanto ninguém escolher outra.
    Other,
}

pub const SWITCHES: [Switch; 4] = [Switch::Shortcut, Switch::AltShift, Switch::CtrlShift, Switch::CapsLock];

impl Switch {
    /// A opção grp: dela (nenhuma para o atalho do perfil e para Other).
    pub fn option(self) -> Option<&'static str> {
        match self {
            Switch::AltShift => Some("grp:alt_shift_toggle"),
            Switch::CtrlShift => Some("grp:ctrl_shift_toggle"),
            Switch::CapsLock => Some("grp:caps_toggle"),
            Switch::Shortcut | Switch::Other => None,
        }
    }
}

/// A tecla de troca de um xkb_options.
pub fn switch_of(options: &[String]) -> Switch {
    let Some(grp) = options.iter().find(|o| o.starts_with("grp:")) else { return Switch::Shortcut };
    SWITCHES.into_iter().find(|s| s.option() == Some(grp.as_str())).unwrap_or(Switch::Other)
}

/// O xkb_options com a tecla escolhida: as opções que não são grp: ficam como estavam, na ordem.
pub fn with_switch(options: &[String], switch: Switch) -> Vec<String> {
    if switch == Switch::Other {
        return options.to_vec();
    }
    let mut out: Vec<String> = options.iter().filter(|o| !o.starts_with("grp:")).cloned().collect();
    out.extend(switch.option().map(str::to_string));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(list: &[&str]) -> Vec<String> {
        list.iter().map(|o| o.to_string()).collect()
    }

    const XML: &str = r#"<xkbConfigRegistry><layoutList>
    <layout><configItem><name>us</name><description>English (US)</description></configItem>
      <variantList>
        <variant><configItem><name>intl</name><description>English (US, intl., with dead keys)</description></configItem></variant>
        <variant><configItem><name>bad name</name><description>Broken</description></configItem></variant>
      </variantList></layout>
    <layout><configItem><name>br</name><!-- Keyboard indicator --><shortDescription>pt</shortDescription>
      <description>Portuguese (Brazil)</description></configItem><variantList/></layout>
    <layout><configItem><name>custom</name><description>A &amp; B</description></configItem></layout>
    </layoutList><optionList><group><configItem><name>grp</name><description>Switching</description></configItem>
      <option><configItem><name>grp:caps_toggle</name><description>Caps Lock</description></configItem></option>
    </group></optionList></xkbConfigRegistry>"#;

    fn choice(layout: &str, variant: &str, description: &str) -> Choice {
        Choice {
            layout: Layout { layout: layout.into(), variant: variant.into() },
            description: description.into(),
        }
    }

    #[test]
    fn the_catalog_lists_layouts_and_variants_by_name() {
        assert_eq!(
            catalog(XML),
            vec![
                choice("custom", "", "A & B"),
                choice("us", "", "English (US)"),
                choice("us", "intl", "English (US, intl., with dead keys)"),
                choice("br", "", "Portuguese (Brazil)"),
            ]
        );
        assert!(catalog("").is_empty());
    }

    #[test]
    fn the_switch_key_is_the_grp_option() {
        assert_eq!(switch_of(&opts(&[])), Switch::Shortcut);
        assert_eq!(switch_of(&opts(&["compose:ralt", "grp:alt_shift_toggle"])), Switch::AltShift);
        assert_eq!(switch_of(&opts(&["grp:ctrl_shift_toggle"])), Switch::CtrlShift);
        assert_eq!(switch_of(&opts(&["grp:caps_toggle"])), Switch::CapsLock);
        assert_eq!(switch_of(&opts(&["grp:win_space_toggle"])), Switch::Other);
        assert_eq!(switch_of(&opts(&["grp_led:scroll"])), Switch::Shortcut, "grp_led: is not a switch");
    }

    #[test]
    fn choosing_a_switch_keeps_the_other_options() {
        let current = opts(&["compose:ralt", "grp:win_space_toggle", "lv3:ralt_switch"]);
        assert_eq!(with_switch(&current, Switch::CapsLock), opts(&["compose:ralt", "lv3:ralt_switch", "grp:caps_toggle"]));
        assert_eq!(with_switch(&current, Switch::Shortcut), opts(&["compose:ralt", "lv3:ralt_switch"]));
        assert_eq!(with_switch(&current, Switch::Other), current, "Other keeps what is there");
        for switch in SWITCHES {
            assert_eq!(switch_of(&with_switch(&current, switch)), switch);
        }
    }
}
