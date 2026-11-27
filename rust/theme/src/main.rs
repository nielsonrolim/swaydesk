//! Lista os temas GTK instalados e troca tema, ícones e cor de destaque em todo o desktop. O porte do
//! `sway-theme` em Python, com os mesmos comandos e as mesmas saídas.
//!
//! uso: sway-theme list | current | families | colors | init
//!      sway-theme icons [<ícones>]
//!      sway-theme set <tema> [--accent #rrggbb] [--folders <cor>] [--icons <ícones>] [--auto | --no-auto]
//!      sway-theme pick <família> light|dark <cor> [--icons <ícones>] [--auto | --no-auto]
//!      sway-theme accent-auto [--print]
//!      sway-theme cursor [<tamanho>]
//!      sway-theme effects [<chave> <valor>]
//! --auto: a cor veio do papel de parede (accent_auto = true); --no-auto desliga o modo. Sem nenhum dos dois, uma
//! cor diferente da atual desliga o modo e a mesma cor o deixa como está.
//! icons: sem nome, os temas de ícones que dá para escolher (o * é o em uso); com nome, troca só os ícones.
//! --icons: os ícones pedidos, no lugar dos que a troca de tema manteria.
//! cursor: sem tamanho, os do tema do cursor (o * é o em uso, ou o mais perto dele); com ele, troca o tamanho.
//! effects: sem chave, o [effects] em uso; com chave e valor (como no config.toml), troca um efeito do SwayFX.
//! accent-auto: com accent_auto = true (o padrão), aplica a cor do papel de parede; --print só mostra o nome dela.
//! --check: só confere que o binário carrega.
use std::process::ExitCode;

use std::collections::HashMap;

use swaydesk_common::config::{self, load_config, message_languages, path, PAPIRUS_COLORS};
use swaydesk_common::i18n::tr;
use swaydesk_common::resident;
use swaydesk_theme::families::{self as sf, Style};
use swaydesk_theme::themes::{self as st, Applied, Auto};
use swaydesk_theme::wallpaper_accent::{wallpaper, wallpaper_color};
use swaydesk_theme::{command_name, desktop as dc, fill, fill_one, icon_themes, papirus as sp};

/// Uma linha das listagens. Com a saída fechada no meio (`| head`), para sem erro, como as ferramentas de
/// sempre; o println! entraria em pânico com o EPIPE.
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(err) = writeln!(std::io::stdout(), $($arg)*) {
            std::process::exit(if err.kind() == std::io::ErrorKind::BrokenPipe { 0 } else { 1 });
        }
    }};
}

/// O nome do processo. Nas mensagens vai o command_name, que nos pacotes é o swaydesk-theme
const PROGRAM: &str = "sway-theme";
const COMMANDS: &str = "{list,current,init,families,colors,icons,cursor,effects,set,pick,accent-auto}";

fn usage_line() -> String {
    format!("usage: {} [-h] {COMMANDS} ...", command_name())
}

/// O erro do argparse: o uso e a mensagem no stderr, com 2.
fn usage_error(message: &str) -> ExitCode {
    eprintln!("{}\n{}: error: {message}", usage_line(), command_name());
    ExitCode::from(2)
}

fn help_text() -> String {
    format!("{}\n\n{}\n\npositional arguments:\n  {COMMANDS}\n\noptions:\n  -h, --help            show this help message and exit\n  --version             show the version and exit",
            usage_line(), tr("Lists the installed GTK themes and changes theme, icons and accent color across the desktop."))
}

fn help() -> ExitCode {
    println!("{}", help_text());
    ExitCode::SUCCESS
}

fn show_list() -> ExitCode {
    let config = load_config(&path()).0;
    let current = dc::effective_gtk_theme(&config);
    for theme in st::discover() {
        let mark = if theme.name == current { "*" } else { " " };
        let kind = if theme.dark { tr("dark") } else { tr("light") };
        let color = theme.accent.clone().unwrap_or_else(|| tr("no color"));
        out!("{mark} {:32} {color:8} {:20} {kind}", theme.name, theme.icons.as_deref().unwrap_or("-"));
    }
    if let Some(note) = st::missing_note(&config) {
        out!("\n{note}");
    }
    ExitCode::SUCCESS
}

fn show_current() -> ExitCode {
    let config = load_config(&path()).0;
    out!("{}", fill(&tr("%(theme)s · icons %(icons)s · accent %(accent)s"),
                        &[("theme", &dc::effective_gtk_theme(&config)), ("icons", &config.icon_theme), ("accent", &config.accent)]));
    if let Some(note) = st::missing_note(&config) {
        out!("{note}");
    }
    ExitCode::SUCCESS
}

fn show_families() -> ExitCode {
    let (families, others) = sf::group(&st::discover());
    for family in &families {
        let styles: Vec<String> = [Style::Light, Style::Dark].into_iter().filter(|s| family.has_style(*s)).map(Style::label).collect();
        let names: Vec<&str> = family.variants.iter().map(|v| v.theme.as_str()).collect();
        out!("{:16} {:14} {}", family.name, styles.join(" "), names.join(", "));
    }
    if !others.is_empty() {
        let names: Vec<&str> = others.iter().map(|t| t.name.as_str()).collect();
        out!("{:16} {:14} {}", tr("Others"), "", names.join(", "));
    }
    ExitCode::SUCCESS
}

fn show_colors() -> ExitCode {
    let config = load_config(&path()).0;
    let installed = sp::available_colors(sp::theme_dir("Papirus").as_deref());
    let current = if sp::is_papirus(&config.icon_theme) { st::folder_color(&config) } else { None };
    for (name, color) in PAPIRUS_COLORS {
        let mark = if current.as_deref() == Some(*name) { "*" } else { " " };
        let missing = if installed.iter().any(|n| n == name) { String::new() } else { format!("  ({})", tr("not installed")) };
        out!("{mark} {name:12} {color}{missing}");
    }
    if installed.is_empty() {
        out!();
        out!("{}", tr("The Papirus icons are not installed: install the papirus-icon-theme package"));
    }
    ExitCode::SUCCESS
}

/// Os temas de ícones que dá para escolher, com o * no em uso (os três Papirus são a entrada "Papirus").
fn show_icons() -> ExitCode {
    let config = load_config(&path()).0;
    let env: HashMap<String, String> = std::env::vars().collect();
    let lang = message_languages(&config, &env).into_iter().next().unwrap_or_default();
    let current = dc::installed_icon_theme(&config);
    let current = icon_themes::entry_of(&current);
    for theme in icon_themes::installed(&lang) {
        let mark = if theme.id == current { "*" } else { " " };
        out!("{mark} {:24} {}", theme.id, theme.label);
    }
    ExitCode::SUCCESS
}

/// Só os ícones: o tema GTK, o destaque, as pastas e o accent_auto ficam como estão. O config é lido dentro da
/// trava: um accent-auto no meio gravaria outra cor, e o apply com a antiga desfaria a dele e desligaria o modo.
fn set_icons(name: &str) -> ExitCode {
    let result = st::locked(|| {
        let config = load_config(&path()).0;
        let theme = dc::effective_gtk_theme(&config);
        st::apply(&theme, Some(&config.accent), Some(&config.folder_color), None, None, Some(name)).map(|applied| (theme, applied))
    });
    match result.and_then(|r| r) {
        Ok((theme, applied)) => report(&theme, applied),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn show_cursor_sizes() -> ExitCode {
    let config = load_config(&path()).0;
    let sizes = swaydesk_common::cursor::theme_sizes(&config.cursor_theme);
    let current = swaydesk_common::cursor::closest_size(&sizes, config.cursor_size);
    for size in sizes {
        out!("{} {size}", if size == current { "*" } else { " " });
    }
    ExitCode::SUCCESS
}

fn show_effects() -> ExitCode {
    for line in st::effects_lines(&load_config(&path()).0.effects) {
        out!("{line}");
    }
    ExitCode::SUCCESS
}

fn set_effect(key: &str, value: &str) -> ExitCode {
    match st::locked(|| st::apply_effect(key, value)).and_then(|r| r) {
        Ok((written, applied)) => {
            println!("{}", fill_one(&tr("Effect: %s"), &format!("{key} = {written}")));
            for note in &applied.notes {
                println!("{note}");
            }
            if !applied.failures.is_empty() {
                eprintln!("{}", fill_one(&tr("Could not update: %s"), &applied.failures.join(", ")));
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn set_cursor_size(value: &str) -> ExitCode {
    // Só algarismos ASCII (o parse aceitaria um "+"); um texto ou um número que não cabe num u32 também é inválido,
    // com a mesma mensagem do tamanho fora da faixa
    let parsed = value.bytes().all(|b| b.is_ascii_digit()).then(|| value.parse::<u32>().ok()).flatten();
    let result = match parsed {
        Some(size) => st::locked(|| st::apply_cursor_size(size)).and_then(|r| r),
        None => Err(swaydesk_theme::Error::Value(fill(&tr("invalid cursor size: %(size)s (use %(min)s to %(max)s)"), &[
            ("size", value),
            ("min", &config::CURSOR_SIZES.start().to_string()),
            ("max", &config::CURSOR_SIZES.end().to_string()),
        ]))),
    };
    match result {
        Ok(applied) => {
            println!("{}", fill_one(&tr("Cursor size: %s"), &parsed.unwrap_or_default().to_string()));
            for note in &applied.notes {
                println!("{note}");
            }
            if !applied.failures.is_empty() {
                eprintln!("{}", fill_one(&tr("Could not update: %s"), &applied.failures.join(", ")));
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn report(theme: &str, applied: Applied) -> ExitCode {
    println!("{}", fill_one(&tr("Theme: %s"), theme));
    for note in &applied.notes {
        println!("{note}");
    }
    if !applied.failures.is_empty() {
        eprintln!("{}", fill_one(&tr("Could not update: %s"), &applied.failures.join(", ")));
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn set_theme(theme: &str, accent: Option<&str>, folders: Option<&str>, auto: Option<bool>, icons: Option<&str>) -> ExitCode {
    match st::locked(|| st::apply(theme, accent, folders, None, auto, icons)).and_then(|r| r) {
        Ok(applied) => report(theme, applied),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn pick_theme(family: &str, style: Style, folders: &str, auto: Option<bool>, icons: Option<&str>) -> ExitCode {
    match st::locked(|| st::pick(family, style, folders, auto, icons)).and_then(|r| r) {
        Ok((theme, applied)) => report(&theme, applied),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

/// A cor do papel de parede, com accent_auto = true (sem cor nele ou sem papel legível, a padrão). Sem o modo,
/// nada, com 0.
fn accent_auto() -> ExitCode {
    match st::locked(st::accent_auto).and_then(|r| r) {
        Ok(Auto::Off) => ExitCode::SUCCESS,
        Ok(Auto::Done { unreadable, applied }) => {
            if unreadable {
                eprintln!("{}", fill_one(&tr("Wallpaper not found or not readable: %s"), &wallpaper().display().to_string()));
            }
            applied.map_or(ExitCode::SUCCESS, |(theme, applied)| report(&theme, applied))
        }
        Err(err) => {
            eprintln!("{}: {err}", command_name());
            ExitCode::from(1)
        }
    }
}

/// Só o nome da cor do Papirus do papel de parede ("none" se ele não tem cor), com ou sem o modo ligado.
fn print_accent() -> ExitCode {
    match wallpaper_color(&wallpaper()) {
        Some(name) => {
            println!("{}", name.unwrap_or("none"));
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("{}", fill_one(&tr("Wallpaper not found or not readable: %s"), &wallpaper().display().to_string()));
            ExitCode::from(1)
        }
    }
}

fn init() -> ExitCode {
    // Na trava: o app pode estar aplicando um tema, e os dois gerariam o tema de ícones ao mesmo tempo
    let result = st::locked(|| st::init_dconf(&load_config(&path()).0));
    // Depois, a cor do papel de parede (o papel pode ter mudado fora do bing-wallpaper). Uma falha aqui só avisa
    if result.is_ok() {
        let _ = accent_auto();
    }
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("{}", tr("Could not write the theme to dconf (gsettings)"));
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("{}: {err}", command_name());
            ExitCode::from(1)
        }
    }
}

/// Os argumentos do `set`, como o argparse: o tema e as opções --accent e --folders, com ou sem o `=` (o
/// --auto e o --no-auto já saíram antes).
fn parse_set(args: &[String]) -> Result<(String, Option<String>, Option<String>), String> {
    let (mut theme, mut accent, mut folders) = (None, None, None);
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let option = |name: &str, i: &mut usize| -> Result<Option<String>, String> {
            if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
                return Ok(Some(value.to_string()));
            }
            if arg == name {
                *i += 1;
                return args.get(*i).filter(|v| !v.starts_with("--")).cloned().map(Some)
                    .ok_or_else(|| format!("argument {name}: expected one argument"));
            }
            Ok(None)
        };
        if let Some(value) = option("--accent", &mut i)? {
            accent = Some(value);
        } else if let Some(value) = option("--folders", &mut i)? {
            folders = Some(value);
        } else if arg.starts_with("--") || theme.is_some() {
            return Err(format!("unrecognized arguments: {arg}"));
        } else {
            theme = Some(arg.clone());
        }
        i += 1;
    }
    let theme = theme.ok_or_else(|| "the following arguments are required: theme".to_string())?;
    Ok((theme, accent, folders))
}

/// Tira o --icons (com ou sem o `=`) dos argumentos do set e do pick, em qualquer posição.
fn take_icons(args: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    let (mut rest, mut icons) = (Vec::new(), None);
    let mut i = 0;
    while i < args.len() {
        if let Some(value) = args[i].strip_prefix("--icons=") {
            icons = Some(value.to_string());
        } else if args[i] == "--icons" {
            i += 1;
            icons = Some(args.get(i).filter(|v| !v.starts_with("--")).cloned().ok_or("argument --icons: expected one argument")?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((rest, icons))
}

fn main() -> ExitCode {
    swaydesk_common::early_args!(help_text());
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    let Some(command) = args.first() else {
        return usage_error("the following arguments are required: command");
    };
    // O --auto e o --no-auto do set e do pick, em qualquer posição depois do comando, um ou outro (o grupo
    // exclusivo do argparse)
    let takes_auto = matches!(command.as_str(), "set" | "pick");
    let has = |flag: &str| takes_auto && args[1..].iter().any(|a| a == flag);
    let auto = match (has("--auto"), has("--no-auto")) {
        (true, true) => return usage_error("argument --no-auto: not allowed with argument --auto"),
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    };
    let rest: Vec<String> = args[1..].iter().filter(|a| !(takes_auto && (*a == "--auto" || *a == "--no-auto"))).cloned().collect();
    let (rest, icons) = if takes_auto {
        match take_icons(&rest) {
            Ok(taken) => taken,
            Err(message) => return usage_error(&message),
        }
    } else {
        (rest, None)
    };
    let (rest, icons) = (&rest[..], icons.as_deref());
    let no_args = |run: fn() -> ExitCode| if rest.is_empty() { run() } else { usage_error(&format!("unrecognized arguments: {}", rest.join(" "))) };
    match command.as_str() {
        "-h" | "--help" => help(),
        "list" => no_args(show_list),
        "current" => no_args(show_current),
        "init" => no_args(init),
        "families" => no_args(show_families),
        "colors" => no_args(show_colors),
        "icons" => match rest {
            [] => show_icons(),
            [name] => set_icons(name),
            _ => usage_error(&format!("unrecognized arguments: {}", rest[1..].join(" "))),
        },
        "set" => match parse_set(rest) {
            Ok((theme, accent, folders)) => set_theme(&theme, accent.as_deref(), folders.as_deref(), auto, icons),
            Err(message) => usage_error(&message),
        },
        "pick" => match rest {
            [family, style, color] => match Style::parse(style) {
                Some(style) => pick_theme(family, style, color, auto, icons),
                None => usage_error(&format!("argument style: invalid choice: '{style}' (choose from 'light', 'dark')")),
            },
            _ if rest.len() < 3 => usage_error("the following arguments are required: family, style, color"),
            _ => usage_error(&format!("unrecognized arguments: {}", rest[3..].join(" "))),
        },
        "cursor" => match rest {
            [] => show_cursor_sizes(),
            [size] => set_cursor_size(size),
            _ => usage_error(&format!("unrecognized arguments: {}", rest[1..].join(" "))),
        },
        "effects" => match rest {
            [] => show_effects(),
            [_] => usage_error("the following arguments are required: value"),
            [key, value] => set_effect(key, value),
            _ => usage_error(&format!("unrecognized arguments: {}", rest[2..].join(" "))),
        },
        "accent-auto" => match rest {
            [] => accent_auto(),
            [print] if print == "--print" => print_accent(),
            _ => usage_error(&format!("unrecognized arguments: {}", rest.join(" "))),
        },
        other => usage_error(&format!("argument command: invalid choice: '{other}' (choose from 'list', 'current', 'init', 'families', 'colors', 'icons', 'cursor', 'effects', 'set', 'pick', 'accent-auto')")),
    }
}
