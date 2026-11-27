//! Tema do Desktop: a família do tema GTK, claro ou escuro, e a cor de destaque, das cores do Papirus. O
//! porte de `home/.local/bin/sway-theme-app`, que passa a vez ao binário swaydesk-theme-app (sway_rust).
//!
//! A barra lateral tem as famílias (Mint-Y, Mint-L, Adwaita, Yaru...) e, depois delas, os outros temas. À
//! direita ficam a prévia, o estilo e as cores, com o interruptor da cor do papel de parede (accent_auto) acima
//! delas. Escolher só muda a prévia. O botão Aplicar, ou Enter, aplica pela biblioteca do sway-theme
//! (swaydesk-theme), e a página continua aberta para testar outra combinação: aplicar reinicia a barra.
//!
//! A mesma página serve a janela do app ([`window`]) e o painel Aparência da Central de Configurações
//! ([`panel`], ligado estaticamente: a Central não depende do pacote do app).
mod choice;
mod cursor;
mod page;
mod preview;

use std::rc::Rc;

use gtk4::prelude::*;

/// A janela do app, com a página dentro; o refresh vem depois de mostrar.
pub fn window(app: &gtk4::Application) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::new(app);
    window.set_title(Some(&swaydesk_common::i18n::tr("Desktop Theme")));
    window.set_default_size(860, 600);
    window.set_icon_name(Some("org.swaydesktop.Theme"));
    let page = page::Page::new(Some(&window));
    window.set_child(Some(&page.root));
    window.present();
    page.refresh(false);
    // A página vive com a janela: o closure do destroy segura a referência até ela fechar
    let keep = std::cell::RefCell::new(Some(page));
    window.connect_destroy(move |_| drop(keep.borrow_mut().take()));
    window
}

/// O painel Aparência da Central: a página sem barra de título. Cada vez que aparece, relê o config (um tema
/// aplicado no app ou pelo terminal com o painel escondido).
pub fn panel() -> gtk4::Widget {
    let page = page::Page::new(None);
    let root = page.root.clone();
    let shown: Rc<page::Page> = page;
    root.connect_map(move |_| {
        shown.on_theme_changed();
    });
    root.upcast()
}
