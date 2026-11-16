//! A lógica do app Telas (swaydesk-displays), sem GTK: as saídas que o Sway informa ([`outputs`]), a
//! configuração de cada uma e os comandos que a aplicam ([`config`]), os perfis por conjunto de monitores
//! ([`profiles`]), a geometria do arrastar ([`geometry`]), o que o serviço faz a cada mudança ([`plan`]) e o
//! arquivo que o Sway lê no login ([`conf`]).
pub mod conf;
pub mod config;
pub mod geometry;
pub mod outputs;
pub mod plan;
pub mod profiles;
