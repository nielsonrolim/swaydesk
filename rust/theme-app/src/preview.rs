//! A prévia: uma miniatura do desktop, com a barra, uma janela nas cores do tema e as pastas na cor. O
//! `Preview` do `sway-theme-app` em Python, desenhado com cairo.
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{cairo, gdk_pixbuf, pango};
use swaydesk_common::config::{self, channels, contrast_with_white, mix, Config};
use swaydesk_common::i18n::tr;
use swaydesk_theme::desktop as dc;
use swaydesk_theme::families::Style;
use swaydesk_theme::papirus as sp;

/// O contraste com o branco acima do qual o fundo é claro (o MID_GREY_CONTRAST do sway_themes)
const MID_GREY_CONTRAST: f64 = 4.58;

pub fn set_rgb(cr: &cairo::Context, color: &str) {
    let [r, g, b] = if color.len() == 7 && color.starts_with('#') { channels(color) } else { [0, 0, 0] };
    cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
}

/// O texto sobre o destaque, como na barra.
pub fn on_accent(accent: &str) -> String {
    config::palette(&Config { accent: accent.to_string(), ..Default::default() }).on_accent
}

/// As cores da prévia dos temas que não as expõem no CSS (Adwaita, Yaru): as do Adwaita.
fn fallback(style: Style, key: &str) -> &'static str {
    match (style, key) {
        (Style::Light, "bg") => "#fafafa",
        (Style::Light, "fg") => "#2e3436",
        (Style::Light, _) => "#ffffff",
        (Style::Dark, "bg") => "#353535",
        (Style::Dark, "fg") => "#eeeeec",
        (Style::Dark, _) => "#2d2d2d",
    }
}

/// O fundo, o texto e a base do tema: os do gtk.css, ou os do Adwaita no estilo.
pub fn theme_colors(theme: Option<&str>, style: Style) -> HashMap<&'static str, String> {
    let read = |name: &str| theme.and_then(|t| dc::theme_color(t, name));
    let mut found: HashMap<&'static str, Option<String>> =
        [("bg", read("theme_bg_color")), ("fg", read("theme_fg_color")), ("base", read("theme_base_color"))].into_iter().collect();
    if found["fg"].is_none() {
        // O Mint-Y não define o texto em hexadecimal
        if let Some(bg) = found["bg"].clone() {
            found.insert("fg", Some(if contrast_with_white(&bg) > MID_GREY_CONTRAST { "#dadada" } else { "#303030" }.to_string()));
        }
    }
    found.into_iter().map(|(key, value)| (key, value.unwrap_or_else(|| fallback(style, key).to_string()))).collect()
}

struct State {
    theme: Option<String>,
    style: Style,
    accent: String,
    color: String,
    /// Os ícones que vão valer: as pastas são as deles (as do Papirus, na cor)
    icon_theme: String,
    /// As cores por tema e estilo: o draw roda a cada redimensionamento
    palettes: HashMap<(Option<String>, Style), HashMap<&'static str, String>>,
    icons: HashMap<(String, String, i32), Option<gdk_pixbuf::Pixbuf>>,
}

pub struct Preview {
    pub area: gtk4::DrawingArea,
    state: Rc<RefCell<State>>,
}

impl Preview {
    pub fn new() -> Rc<Preview> {
        let area = gtk4::DrawingArea::new();
        area.set_content_height(210);
        let state = Rc::new(RefCell::new(State {
            theme: None,
            style: Style::Dark,
            accent: "#16a085".into(),
            color: "teal".into(),
            icon_theme: String::new(),
            palettes: HashMap::new(),
            icons: HashMap::new(),
        }));
        let weak = Rc::downgrade(&state);
        area.set_draw_func(move |area, cr, width, height| {
            if let Some(state) = weak.upgrade() {
                draw(area, cr, width as f64, height as f64, &mut state.borrow_mut());
            }
        });
        Rc::new(Preview { area, state })
    }

    pub fn set_choice(&self, theme: Option<String>, style: Style, accent: &str, color: &str, icons: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.theme = theme;
            state.style = style;
            state.accent = accent.to_string();
            state.color = color.to_string();
            state.icon_theme = icons.to_string();
        }
        self.area.queue_draw();
    }
}

/// O ícone da pasta: com os ícones Papirus (ou sem saber quais), o do Papirus na cor; com outros, a pasta deles.
/// Sem nenhum dos dois, a pasta do tema de ícones em uso.
fn folder(area: &gtk4::DrawingArea, icons: &str, color: &str, size: i32) -> Option<gdk_pixbuf::Pixbuf> {
    let papirus = icons.is_empty() || icons == "?" || sp::is_papirus(icons);
    if papirus {
        if let Some(directory) = sp::theme_dir("Papirus") {
            let path = directory.join("48x48/places").join(format!("folder-{color}.svg"));
            if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_size(&path, size, size) {
                return Some(pixbuf);
            }
        }
    } else {
        let theme = gtk4::IconTheme::new();
        theme.set_theme_name(Some(icons));
        if theme.has_icon("folder") {
            let icon = theme.lookup_icon("folder", &[], size, 1, gtk4::TextDirection::None, gtk4::IconLookupFlags::empty());
            if let Some(pixbuf) = icon.file().and_then(|f| f.path()).and_then(|p| gdk_pixbuf::Pixbuf::from_file_at_size(p, size, size).ok()) {
                return Some(pixbuf);
            }
        }
    }
    let theme = gtk4::IconTheme::for_display(&area.display());
    let icon = theme.lookup_icon("folder", &[], size, 1, gtk4::TextDirection::None, gtk4::IconLookupFlags::empty());
    let path = icon.file()?.path()?;
    gdk_pixbuf::Pixbuf::from_file_at_size(path, size, size).ok()
}

fn rounded(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -PI / 2.0, 0.0);
    cr.arc(x + width - radius, y + height - radius, radius, 0.0, PI / 2.0);
    cr.arc(x + radius, y + height - radius, radius, PI / 2.0, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn text(area: &gtk4::DrawingArea, cr: &cairo::Context, x: f64, y: f64, text: &str, color: &str, size: u32, bold: bool) {
    let layout = area.create_pango_layout(Some(text));
    let description = pango::FontDescription::from_string(&format!("Sans {}{size}", if bold { "Bold " } else { "" }));
    layout.set_font_description(Some(&description));
    set_rgb(cr, color);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}

fn fill(cr: &cairo::Context, color: &str) {
    set_rgb(cr, color);
    let _ = cr.fill();
}

fn draw(area: &gtk4::DrawingArea, cr: &cairo::Context, width: f64, height: f64, state: &mut State) {
    let key = (state.theme.clone(), state.style);
    if !state.palettes.contains_key(&key) {
        let colors = theme_colors(state.theme.as_deref(), state.style);
        state.palettes.insert(key.clone(), colors);
    }
    let colors = state.palettes[&key].clone();
    let (bg, fg) = (colors["bg"].as_str(), colors["fg"].as_str());
    let accent = state.accent.clone();
    let on = on_accent(&accent);
    let dark = state.style == Style::Dark;
    // O fundo da área de trabalho
    rounded(cr, 0.0, 0.0, width, height, 10.0);
    fill(cr, &mix(&accent, "#101010", 0.82));
    // A barra: sempre escura, com o destaque no espaço de trabalho ativo
    let _ = cr.save();
    rounded(cr, 0.0, 0.0, width, height, 10.0);
    cr.clip();
    cr.rectangle(0.0, 0.0, width, 22.0);
    fill(cr, "#1d1d20");
    let _ = cr.restore();
    for index in 0..4 {
        rounded(cr, 10.0 + index as f64 * 24.0, 5.0, 18.0, 12.0, 4.0);
        fill(cr, if index == 0 { &accent } else { "#3a3a40" });
    }
    text(area, cr, width - 52.0, 4.0, "12:00", "#e8e8e8", 8, false);
    // A janela
    let (wx, wy, ww, wh) = (18.0, 36.0, width - 36.0, height - 50.0);
    rounded(cr, wx, wy, ww, wh, 8.0);
    fill(cr, bg);
    let header = mix(bg, if dark { "#ffffff" } else { "#000000" }, 0.06);
    let _ = cr.save();
    rounded(cr, wx, wy, ww, wh, 8.0);
    cr.clip();
    cr.rectangle(wx, wy, ww, 30.0);
    fill(cr, &header);
    // A lateral da janela, com o item selecionado no destaque
    let side = 150f64.min((ww / 3.0).floor());
    cr.rectangle(wx, wy + 30.0, side, wh - 30.0);
    fill(cr, &mix(bg, fg, 0.04));
    let _ = cr.restore();
    text(area, cr, wx + 12.0, wy + 7.0, &tr("Documents"), fg, 9, true);
    for (index, label) in [tr("Home"), tr("Documents"), tr("Downloads")].iter().enumerate() {
        let ry = wy + 40.0 + index as f64 * 26.0;
        if index == 1 {
            rounded(cr, wx + 6.0, ry - 3.0, side - 12.0, 22.0, 5.0);
            fill(cr, &accent);
        }
        text(area, cr, wx + 16.0, ry, label, if index == 1 { &on } else { fg }, 9, false);
    }
    // As pastas dos ícones escolhidos: as do Papirus na cor, as dos outros como são
    let size = 44;
    let icon_key = (state.icon_theme.clone(), state.color.clone(), size);
    if !state.icons.contains_key(&icon_key) {
        let icon = folder(area, &state.icon_theme, &state.color, size);
        state.icons.insert(icon_key.clone(), icon);
    }
    let icon = state.icons[&icon_key].clone();
    let content_x = wx + side + 16.0;
    for (index, label) in [tr("Music"), tr("Pictures"), tr("Projects")].iter().enumerate() {
        let ix = content_x + index as f64 * (size as f64 + 30.0);
        if ix + size as f64 > wx + ww - 8.0 {
            break;
        }
        if let Some(icon) = &icon {
            cr.set_source_pixbuf(icon, ix + 8.0, wy + 42.0);
            let _ = cr.paint();
        }
        text(area, cr, ix + 4.0, wy + 42.0 + size as f64 + 2.0, label, fg, 8, false);
    }
    // Os controles: um botão de ação, uma caixa marcada e um interruptor
    let by = wy + wh - 40.0;
    rounded(cr, content_x, by, 92.0, 26.0, 5.0);
    fill(cr, &accent);
    text(area, cr, content_x + 22.0, by + 5.0, &tr("Apply"), &on, 9, true);
    rounded(cr, content_x + 106.0, by + 5.0, 16.0, 16.0, 3.0);
    fill(cr, &accent);
    set_rgb(cr, &on);
    cr.set_line_width(2.0);
    cr.move_to(content_x + 109.0, by + 13.0);
    cr.line_to(content_x + 113.0, by + 17.0);
    cr.line_to(content_x + 119.0, by + 9.0);
    let _ = cr.stroke();
    let sx = content_x + 136.0;
    if sx + 40.0 < wx + ww {
        rounded(cr, sx, by + 4.0, 38.0, 18.0, 9.0);
        fill(cr, &accent);
        cr.arc(sx + 29.0, by + 13.0, 7.0, 0.0, 2.0 * PI);
        fill(cr, "#ffffff");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_without_colors_uses_adwaita() {
        let colors = theme_colors(None, Style::Dark);
        assert_eq!((colors["bg"].as_str(), colors["fg"].as_str(), colors["base"].as_str()), ("#353535", "#eeeeec", "#2d2d2d"));
        assert_eq!(theme_colors(None, Style::Light)["bg"], "#fafafa");
    }
}
