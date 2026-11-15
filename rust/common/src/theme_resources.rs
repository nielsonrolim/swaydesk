//! Os recursos dos temas GTK do sistema (o Yaru do Ubuntu guarda o CSS num gtk.gresource). O GTK só
//! registra o do tema em uso ao abrir: trocado por fora com um programa aberto (de um tema que não é Yaru
//! para um Yaru), o recurso novo não estava registrado, o import falhava ("resource
//! /com/ubuntu/themes/<Yaru-x>/4.0/gtk.css does not exist") e a janela podia ficar transparente (visto no
//! Ubuntu pela vm-manager, #111). Os caminhos são separados por tema, então registrar todos não muda o tema
//! em uso. Chame antes do gtk4::init (ou do Application::new).

/// Registra o gtk-4.0/gtk.gresource de cada tema em /usr/share/themes. Um que traga caminhos do próprio
/// GTK (/org/gtk/...) fica de fora: o registro entra na frente da lista e passaria a valer mesmo sem o tema
/// em uso.
pub fn register_theme_resources() {
    let Ok(entries) = std::fs::read_dir("/usr/share/themes") else { return };
    for entry in entries.flatten() {
        let path = entry.path().join("gtk-4.0/gtk.gresource");
        if !path.is_file() {
            continue;
        }
        if let Ok(resource) = gio::Resource::load(&path) {
            if resource.enumerate_children("/org/gtk", gio::ResourceLookupFlags::NONE).is_err() {
                gio::resources_register(&resource);
            }
        }
    }
}
