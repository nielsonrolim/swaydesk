//! O que os componentes compartilham, sem GTK. Cada módulo espelha um dos módulos em Python de
//! `home/.local/bin`: `config` o `desktop_config.py`, `i18n` o `sway_i18n.py`, `resident` o
//! `waybar_common.py` e `text` as funções de `waybar_clipboard.py`. `theme_resources` registra os recursos dos
//! temas GTK do sistema, e `renderer` escolhe o renderizador do GTK 4 de todos os componentes: são o que
//! traz o gio (não o GTK) para cá. `power`: os perfis de energia e se a máquina tem bateria.
pub mod app_scope;
pub mod cli;
pub mod config;
pub mod cursor;
pub mod dirs;
pub mod paths;
pub mod power;
pub mod reference;
pub mod i18n;
pub mod keyboard;
pub mod offline_updates;
pub mod renderer;
pub mod resident;
pub mod theme_resources;
pub mod text;
