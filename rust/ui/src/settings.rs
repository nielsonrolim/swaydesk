//! Como um painel entra na Central de Configurações (swaydesk-settings, 0.2.0): uma função que monta o widget
//! dele. O crate de um app (o Tema, as Telas) a oferece numa biblioteca, e a Central a liga estaticamente: o
//! pacote da Central não depende do pacote do app, e o app continua com a sua janela própria.

/// Um painel: o id da lista do swaydesk-settings-logic e a função que monta o conteúdo, chamada só na primeira
/// vez que o painel aparece.
#[derive(Clone, Copy)]
pub struct Panel {
    pub id: &'static str,
    pub build: fn() -> gtk4::Widget,
}
