//! O "Identificar": o nome e o modelo de cada tela, grandes, no meio dela, por alguns segundos.
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use swaydesk_displays_logic::outputs::Output;

const SECONDS: u32 = 3;
const CSS: &str = r#"
.displays-identify-window { background: transparent; }
.displays-identify { background: rgba(20, 20, 24, 0.88); border-radius: 18px; padding: 18px 28px; }
.displays-identify label { color: #ffffff; }
.displays-identify .name { font-size: 40px; font-weight: bold; }
.displays-identify .model { font-size: 16px; }
"#;

pub fn show(outputs: &[Output]) {
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let connector = monitor.connector().map(|c| c.to_string()).unwrap_or_default();
        let Some(output) = outputs.iter().find(|o| o.name == connector) else { continue };
        let window = gtk4::Window::new();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_namespace("swaydesk-displays-identify");
        window.add_css_class("displays-identify-window");
        window.set_monitor(&monitor);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        card.add_css_class("displays-identify");
        let name = gtk4::Label::new(Some(&output.name));
        name.add_css_class("name");
        card.append(&name);
        let description = output.description();
        if !description.is_empty() {
            let model = gtk4::Label::new(Some(&description));
            model.add_css_class("model");
            card.append(&model);
        }
        window.set_child(Some(&card));
        window.present();
        glib::timeout_add_seconds_local_once(SECONDS, move || window.destroy());
    }
}
