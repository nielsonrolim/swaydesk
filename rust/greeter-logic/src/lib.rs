//! A lógica da tela de login (swaydesk-greeter), sem GTK: a conversa com o greetd ([`ipc`]), as sessões
//! instaladas ([`sessions`]), as contas que aparecem ([`users`]), a memória do último login ([`state`]) e o
//! que vem do sistema: idioma, config e teclado ([`system`]).
pub mod ipc;
pub mod sessions;
pub mod state;
pub mod system;
pub mod users;
