//! Mapeamentos do usuário: `:map`, `:nmap`, `:imap`, `:vmap` (e as versões `noremap`/`unmap`).
//! A tabela vale para o editor todo; um prefixo pendente (`<Leader>g` com `<Leader>gs` mapeado)
//! espera a próxima tecla. Sem casamento, as teclas seguem como foram digitadas.

use super::*;

pub const MODO_N: u8 = 1;
pub const MODO_I: u8 = 2;
pub const MODO_V: u8 = 4;
pub const MODO_TODOS: u8 = MODO_N | MODO_I | MODO_V;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapa {
    pub modos: u8,
    pub lhs: Vec<Key>,
    pub rhs: Vec<Key>,
    pub recursivo: bool,
    /// texto como foi escrito (para listar)
    pub lhs_texto: String,
    pub rhs_texto: String,
}

/// Profundidade máxima de mapeamentos recursivos (`maxmapdepth`).
const PROFUNDIDADE: usize = 100;

impl Editor {
    /// Tecla líder usada em `<Leader>` nos mapeamentos.
    pub fn set_leader(&mut self, c: char) {
        self.leader = c;
    }

    /// `<Leader>` e `<LocalLeader>` viram a tecla escolhida.
    fn expandir_leader(&self, s: &str) -> String {
        let l = if self.leader == ' ' { "<Space>".to_string() } else if self.leader == '<' { "<lt>".to_string() } else { self.leader.to_string() };
        let mut out = String::new();
        let mut resto = s;
        while let Some(i) = resto.find('<') {
            out.push_str(&resto[..i]);
            let tail = &resto[i..];
            let low = tail.to_lowercase();
            if low.starts_with("<leader>") {
                out.push_str(&l);
                resto = &tail[8..];
            } else if low.starts_with("<localleader>") {
                out.push_str(&l);
                resto = &tail[13..];
            } else {
                out.push('<');
                resto = &tail[1..];
            }
        }
        out.push_str(resto);
        out
    }

    /// Cria ou troca um mapeamento nos `modos` (`MODO_N` etc. combinados).
    pub fn mapear(&mut self, modos: u8, lhs: &str, rhs: &str, recursivo: bool) {
        let l = self.expandir_leader(lhs);
        let r = self.expandir_leader(rhs);
        let (lk, rk) = (crate::keys::parse_keys(&l), crate::keys::parse_keys(&r));
        if lk.is_empty() {
            return;
        }
        // o mesmo lhs num modo só vale uma vez: tira o modo dos antigos
        for m in self.mapas.iter_mut().filter(|m| m.lhs == lk) {
            m.modos &= !modos;
        }
        self.mapas.retain(|m| m.modos != 0);
        self.mapas.push(Mapa { modos, lhs: lk, rhs: rk, recursivo, lhs_texto: lhs.to_string(), rhs_texto: rhs.to_string() });
        self.mapas_versao += 1;
    }

    /// Tira o mapeamento de `lhs` dos `modos`; `false` se não existia.
    pub fn desmapear(&mut self, modos: u8, lhs: &str) -> bool {
        let lk = crate::keys::parse_keys(&self.expandir_leader(lhs));
        let mut achou = false;
        for m in self.mapas.iter_mut().filter(|m| m.lhs == lk && m.modos & modos != 0) {
            m.modos &= !modos;
            achou = true;
        }
        self.mapas.retain(|m| m.modos != 0);
        if achou {
            self.mapas_versao += 1;
        }
        achou
    }

    pub fn limpar_mapas(&mut self, modos: u8) {
        for m in self.mapas.iter_mut() {
            m.modos &= !modos;
        }
        self.mapas.retain(|m| m.modos != 0);
        self.mapas_versao += 1;
    }

    /// Para listar com `:map`: `n  <Space>w  :w<CR>`.
    pub fn listar_mapas(&self) -> Vec<String> {
        self.mapas
            .iter()
            .map(|m| {
                let modos: String = [(MODO_N, 'n'), (MODO_I, 'i'), (MODO_V, 'v')].iter().map(|(b, c)| if m.modos & b != 0 { *c } else { ' ' }).collect();
                format!("{modos}  {}  {}{}", m.lhs_texto, if m.recursivo { " " } else { "*" }, m.rhs_texto)
            })
            .collect()
    }

    /// Muda quando a tabela muda (o espaço de trabalho copia para os outros editores).
    pub fn mapas_versao(&self) -> u64 {
        self.mapas_versao
    }

    pub fn mapas(&self) -> Vec<Mapa> {
        self.mapas.clone()
    }

    pub fn definir_mapas(&mut self, mapas: Vec<Mapa>, versao: u64) {
        self.mapas = mapas;
        self.mapas_versao = versao;
    }

    /// Teclas de um mapeamento que ainda espera a próxima (para mostrar na tela).
    pub fn mapa_pendente(&self) -> String {
        self.map_pend.iter().map(|k| match k {
            Key::Char(c) => c.to_string(),
            Key::Esc => "^[".into(),
            Key::Enter => "^M".into(),
            Key::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
            _ => String::new(),
        }).collect()
    }

    fn modo_do_mapa(&self) -> u8 {
        match self.mode {
            Mode::Normal => MODO_N,
            Mode::Insert => MODO_I,
            Mode::Visual => MODO_V,
        }
    }

    /// Passa a tecla pela tabela de mapeamentos. `true`: já tratada (esperando mais teclas ou expandida).
    pub(super) fn tentar_mapa(&mut self, k: Key) -> bool {
        if self.mapas.is_empty() && self.map_pend.is_empty() {
            return false;
        }
        // não mapeia o que está no meio de um comando (operador pendente, linha de comando, `q`, `<C-r>`)
        let ocupado = self.replaying || self.cmdline.is_some() || self.q_wait || self.ins_reg_wait || (self.mode != Mode::Insert && !self.pending.is_empty());
        if ocupado && self.map_pend.is_empty() {
            return false;
        }
        let bit = self.modo_do_mapa();
        let mut cand = self.map_pend.clone();
        cand.push(k);
        let exato = self.mapas.iter().find(|m| m.modos & bit != 0 && m.lhs == cand).cloned();
        let mais_longo = self.mapas.iter().any(|m| m.modos & bit != 0 && m.lhs.len() > cand.len() && m.lhs.starts_with(&cand));
        if mais_longo {
            self.map_pend = cand;
            return true;
        }
        if let Some(m) = exato {
            self.map_pend.clear();
            if self.map_prof >= PROFUNDIDADE {
                self.failed = true;
                return true;
            }
            self.map_prof += 1;
            for rk in &m.rhs {
                if m.recursivo {
                    self.feed_one(*rk);
                } else {
                    self.feed_one_cru(*rk);
                }
            }
            self.map_prof -= 1;
            return true;
        }
        if self.map_pend.is_empty() {
            return false;
        }
        // prefixo que não virou nada: a primeira tecla segue como digitada e as outras tentam de novo
        let mut resto = std::mem::take(&mut self.map_pend);
        resto.push(k);
        let primeiro = resto.remove(0);
        self.feed_one_cru(primeiro);
        for r in resto {
            self.feed_one(r);
        }
        true
    }
}
