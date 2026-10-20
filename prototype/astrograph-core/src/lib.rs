//! Núcleo do editor modal do Astrograph, sem Neovim: buffer, teclas, movimentos,
//! operadores, registradores, desfazer e repetição (`.`).
//!
//! O comportamento é o do Vim/Neovim, conferido por teste diferencial: as mesmas
//! teclas rodam no `nvim --headless` e aqui, e o texto e o cursor têm de bater
//! (`examples/diferencial.rs`).

pub mod buffer;
pub mod editor;
pub mod keys;
pub mod motion;

pub use buffer::{Buffer, Pos};
pub use editor::{Editor, Mode};
pub use keys::{parse_keys, Key};
