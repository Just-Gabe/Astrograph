//! Servicos do Astrograph em Rust: o que antes vivia em Lua dentro do editor.
//! O app usa esta biblioteca direto; o editor do terminal chama o executavel
//! `astrograph-servicos` (mesma logica, saida em JSON).

pub mod atualizar;
pub mod nativo;
pub mod proc;
pub mod projeto;
