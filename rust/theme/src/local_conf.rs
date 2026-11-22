//! O modelo do `~/.config/sway/config.d/50-local.conf` de uma conta nova, na língua da sessão: os comentários
//! são textos do catálogo (os msgid em inglês), e os exemplos de config ficam como estão. O
//! `home-copy/.config/sway/config.d/50-local.conf` do repositório (o do `install.sh` e o skel do pacote) é
//! este modelo em português, e um teste confere que os dois não se afastam.
//!
//! Cada comentário é um msgid só, com as quebras de linha dentro: quem traduz quebra as linhas na sua língua
//! (até ~110 colunas). Cada chamada ao tr leva o texto literal, para o `po/update` (xgettext) achar os textos.

/// O modelo inteiro, com os textos traduzidos por `tr` (o `i18n::tr` da sessão, ou um catálogo nos testes).
pub fn template(tr: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let comment = |out: &mut String, text: String| {
        for line in text.lines() {
            out.push_str(if line.is_empty() { "#" } else { "# " });
            out.push_str(line);
            out.push('\n');
        }
    };
    // Um título ou um nome numa linha só: uma quebra de linha na tradução deixaria uma linha sem o #, que o
    // Sway leria como config
    let heading = |out: &mut String, text: String| out.push_str(&format!("\n# --- {} ---\n", one_line(&text)));
    let raw = |out: &mut String, lines: &[&str]| {
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    };

    comment(&mut out, tr("Settings for this machine only: shortcuts, displays, keyboard, mouse and apps. Sway reads this file\nafter the Swaydesk shortcuts (and the chosen shortcut profile), so what is here takes precedence."));
    comment(&mut out, tr("The file is yours: neither the installation nor the updates overwrite it. To go back to the template,\ndelete it and log in again."));
    comment(&mut out, tr("Uncomment what you want and run swaymsg reload in a terminal. An error here shows up in a Sway bar at\nthe top of the screen, with the line of the problem."));
    comment(&mut out, tr("The Swaydesk Help (Super+F1) shows the shortcuts in use, with the changes of this file already in. A\n\"##\" comment right above a shortcut is the description shown there."));

    heading(&mut out, tr("Shortcuts: change what a shortcut does"));
    comment(&mut out, tr("Use --no-warn: without it, Sway warns that the key already had a shortcut."));
    raw(&mut out, &["# ## Open foot", "# bindsym --no-warn Ctrl+Alt+t exec foot"]);

    heading(&mut out, tr("Shortcuts: remove a shortcut"));
    comment(&mut out, tr("Only remove what the profile in use has (see the Help), written as it defines it, including options\nsuch as --locked (volume and brightness, which work with the screen locked). Removing a key with no\nshortcut, or without its option, is a config error. The Print key exists in every profile, with no options."));
    raw(&mut out, &["# unbindsym Print", "# unbindsym --locked XF86AudioRaiseVolume"]);

    heading(&mut out, tr("Shortcuts: create a shortcut"));
    comment(&mut out, tr("Pick a free key (the Help shows the ones in use). Mod4 is the Super key. Super+F2 to F12 and\nSuper+Ctrl+Alt+<letter> are free in every profile."));
    raw(&mut out, &["# ## Open another terminal", "# bindsym Mod4+F12 exec kitty"]);

    heading(&mut out, tr("Display: the output name comes from `swaymsg -t get_outputs`"));
    comment(&mut out, tr("The Displays app saves the displays in config.d/90-displays.conf, read after this file: what you\nchoose there takes precedence over an output line here."));
    raw(&mut out, &["# output HDMI-A-1 mode 1920x1080@60Hz scale 1"]);

    heading(&mut out, tr("Keyboard: two layouts (the switching shortcut is in the Help: Super+Space in the swaydesk profile)"));
    raw(&mut out, &["# input type:keyboard {", "#     xkb_layout  br,us", "#     xkb_variant abnt2,intl", "# }"]);

    heading(&mut out, tr("Mouse"));
    raw(&mut out, &["# input type:pointer {", "#     left_handed    enabled", "#     natural_scroll enabled", "# }"]);

    heading(&mut out, tr("Programs when the session starts"));
    // TRANSLATORS: the name of an example program in the 50-local.conf template (exec my-program), with no spaces
    raw(&mut out, &[&format!("# exec {}", one_line(&tr("my-program")).replace(' ', "-"))]);

    heading(&mut out, tr("Apps on workspaces"));
    raw(&mut out, &["# assign [app_id=\"^kitty$\"] workspace number 3"]);
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use swaydesk_common::i18n;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn translated(language: &str) -> String {
        let catalog = i18n::load_catalog(&[language.to_string()], &[repo().join("home/.local/share/locale")]);
        template(&|msgid| catalog.gettext(msgid).to_string())
    }

    // O modelo do repositório (install.sh e skel) é o deste módulo em português, byte a byte
    #[test]
    fn the_portuguese_template_is_the_one_of_the_repository() {
        let file = std::fs::read_to_string(repo().join("home-copy/.config/sway/config.d/50-local.conf")).unwrap();
        assert_eq!(translated("pt_BR.UTF-8"), file);
    }

    #[test]
    fn english_without_a_catalog_and_spanish_with_one() {
        let english = translated("en_US.UTF-8");
        assert!(english.starts_with("# Settings for this machine only: shortcuts, displays"), "{english}");
        assert!(english.contains("\n# --- Mouse ---\n# input type:pointer {\n"), "{english}");
        assert!(english.ends_with("# exec my-program\n\n# --- Apps on workspaces ---\n# assign [app_id=\"^kitty$\"] workspace number 3\n"), "{english}");
        assert_eq!(translated("de_DE.UTF-8"), english);
        let spanish = translated("es_ES.UTF-8");
        assert!(spanish.starts_with("# Ajustes solo de esta máquina"), "{spanish}");
        // As linhas de config são as mesmas em todas as línguas, na mesma ordem (fora a do exec, com o nome do
        // programa traduzido)
        const CONFIG: [&str; 8] = ["# ##", "# bindsym", "# unbindsym", "# output", "# input", "#     ", "# }", "# assign"];
        let examples = |text: &str| text.lines().filter(|l| CONFIG.iter().any(|p| l.starts_with(p))).map(str::to_string).collect::<Vec<_>>();
        assert_eq!(examples(&english).len(), 16, "{english}");
        assert_eq!(examples(&spanish), examples(&english));
        assert_eq!(examples(&translated("pt_BR")), examples(&english));
    }

    // Uma tradução com quebra de linha num título ou no nome do programa continua em uma linha comentada
    #[test]
    fn a_line_break_in_a_heading_stays_a_comment() {
        let text = template(&|msgid| if msgid.contains('\n') { msgid.to_string() } else { format!("{msgid}\nbroken here") });
        for line in text.lines() {
            assert!(line.is_empty() || line.starts_with('#'), "{line}");
        }
        assert!(text.contains("# exec my-program-broken-here\n"), "{text}");
    }

    // Todo comentário é uma linha "# texto" (ou "#"): o Sway leria uma linha sem o # como config
    #[test]
    fn every_line_is_a_comment_and_fits() {
        for language in ["en_US", "pt_BR", "es_ES"] {
            for line in translated(language).lines() {
                assert!(line.is_empty() || line.starts_with('#'), "{language}: {line}");
                assert!(line.chars().count() <= 112, "{language}: {} columns: {line}", line.chars().count());
            }
        }
    }
}
