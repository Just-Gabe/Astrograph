//! Central de configurações: uma lista (a da tela inicial, `c`) com cada preferência e o valor atual. `Enter`
//! troca o valor, grava no arquivo de configuração (sem apagar comentários) e, quando dá, aplica na hora.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ajuste {
    Tema,
    Fonte,
    Quebra,
    Dicas,
    AnimCursor,
    AnimRolagem,
    Relogio,
    Hora12,
    Formatar,
    Controle,
    Motor,
    Lider,
    ArquivoConfig,
    Agente,
    ModoAgente,
    Atualizar,
    Saude,
    Atalhos,
}

pub(super) const AJUSTES: [Ajuste; 18] = [
    Ajuste::Tema,
    Ajuste::Fonte,
    Ajuste::Quebra,
    Ajuste::Dicas,
    Ajuste::AnimCursor,
    Ajuste::AnimRolagem,
    Ajuste::Relogio,
    Ajuste::Hora12,
    Ajuste::Formatar,
    Ajuste::Controle,
    Ajuste::Motor,
    Ajuste::Lider,
    Ajuste::ArquivoConfig,
    Ajuste::Agente,
    Ajuste::ModoAgente,
    Ajuste::Atualizar,
    Ajuste::Saude,
    Ajuste::Atalhos,
];

const FONTES: [f32; 8] = [16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 32.0];
const ANIM_CURSOR: [u64; 5] = [0, 40, 80, 120, 160];
const ANIM_ROLAGEM: [u64; 4] = [0, 100, 160, 240];
const LIDERES: [&str; 4] = ["space", ",", "\\", ";"];

fn sim_nao(b: bool) -> String {
    if b { "ligado" } else { "desligado" }.into()
}

fn ms(n: u64) -> String {
    if n == 0 { "sem animação".into() } else { format!("{n} ms") }
}

/// O próximo valor de uma lista circular (o que vem depois do atual; o primeiro se o atual não está nela).
fn proximo<T: PartialEq + Copy>(lista: &[T], atual: T) -> T {
    match lista.iter().position(|v| *v == atual) {
        Some(i) => lista[(i + 1) % lista.len()],
        None => lista[0],
    }
}

impl Ajuste {
    pub(super) fn rotulo(self) -> &'static str {
        match self {
            Ajuste::Tema => "Tema",
            Ajuste::Fonte => "Tamanho da fonte",
            Ajuste::Quebra => "Quebra de linha",
            Ajuste::Dicas => "Dicas de atalhos",
            Ajuste::AnimCursor => "Animação do cursor",
            Ajuste::AnimRolagem => "Rolagem suave",
            Ajuste::Relogio => "Relógio",
            Ajuste::Hora12 => "Relógio em 12 horas",
            Ajuste::Formatar => "Formatar ao salvar (LSP)",
            Ajuste::Controle => "Agentes operam o editor (MCP)",
            Ajuste::Motor => "Motor de edição",
            Ajuste::Lider => "Tecla líder",
            Ajuste::ArquivoConfig => "Abrir o arquivo de configuração",
            Ajuste::Agente => "Agente de IA",
            Ajuste::ModoAgente => "Modo do agente",
            Ajuste::Atualizar => "Atualizar o Astrograph",
            Ajuste::Saude => "Saúde do ambiente",
            Ajuste::Atalhos => "Atalhos da tecla líder",
        }
    }

    /// O valor mostrado à direita do rótulo.
    pub(super) fn valor(self, c: &crate::config::Config) -> String {
        match self {
            Ajuste::Tema => match c.theme.mode.as_str() {
                "light" => "claro".into(),
                "auto" => "automático".into(),
                _ => "escuro".into(),
            },
            Ajuste::Fonte => format!("{} px", c.font.size),
            Ajuste::Quebra => sim_nao(c.editor.wrap),
            Ajuste::Dicas => sim_nao(c.ui.tips),
            Ajuste::AnimCursor => ms(c.cursor.animation_ms),
            Ajuste::AnimRolagem => ms(c.scroll.animation_ms),
            Ajuste::Relogio => sim_nao(c.clock.enabled),
            Ajuste::Hora12 => sim_nao(c.clock.hour12),
            Ajuste::Formatar => sim_nao(c.lsp.format_on_save),
            Ajuste::Controle => sim_nao(c.agentes.controle),
            Ajuste::Motor => if c.engine.native { "nativo (Rust)" } else { "Neovim" }.into(),
            Ajuste::Lider => tecla_nome(c.keys.leader_char()),
            Ajuste::ArquivoConfig | Ajuste::Agente | Ajuste::ModoAgente | Ajuste::Atualizar | Ajuste::Saude | Ajuste::Atalhos => "abrir".into(),
        }
    }

    /// `[seção] chave = novo valor` e se o efeito aparece na hora (`false`: só ao reabrir). `None` para as ações.
    pub(super) fn trocar(self, c: &crate::config::Config) -> Option<(&'static str, &'static str, toml_edit::Value, bool)> {
        Some(match self {
            Ajuste::Tema => {
                let novo = match c.theme.mode.as_str() {
                    "dark" | "" => "light",
                    "light" => "auto",
                    _ => "dark",
                };
                ("theme", "mode", novo.into(), novo != "auto")
            }
            Ajuste::Fonte => ("font", "size", (proximo(&FONTES, c.font.size) as f64).into(), false),
            Ajuste::Quebra => ("editor", "wrap", (!c.editor.wrap).into(), true),
            Ajuste::Dicas => ("ui", "tips", (!c.ui.tips).into(), true),
            Ajuste::AnimCursor => ("cursor", "animation_ms", (proximo(&ANIM_CURSOR, c.cursor.animation_ms) as i64).into(), false),
            Ajuste::AnimRolagem => ("scroll", "animation_ms", (proximo(&ANIM_ROLAGEM, c.scroll.animation_ms) as i64).into(), false),
            Ajuste::Relogio => ("clock", "enabled", (!c.clock.enabled).into(), false),
            Ajuste::Hora12 => ("clock", "hour12", (!c.clock.hour12).into(), false),
            Ajuste::Formatar => ("lsp", "format_on_save", (!c.lsp.format_on_save).into(), true),
            Ajuste::Controle => ("agentes", "controle", (!c.agentes.controle).into(), true),
            Ajuste::Motor => ("engine", "native", (!c.engine.native).into(), false),
            Ajuste::Lider => {
                let atual = LIDERES.iter().find(|l| crate::config::KeysCfg { leader: (**l).into(), ..Default::default() }.leader_char() == c.keys.leader_char()).copied().unwrap_or("space");
                ("keys", "leader", proximo(&LIDERES, atual).into(), false)
            }
            _ => return None,
        })
    }
}

impl Session {
    /// A central de configurações. `sel`: a linha que fica destacada (ao trocar um valor a lista volta no mesmo lugar).
    pub(super) fn configuracoes(&mut self, sel: usize) {
        let c = crate::config::atual();
        let itens: Vec<ListaItem> = AJUSTES.iter().map(|a| ListaItem { texto: a.rotulo().into(), detalhe: a.valor(&c), destino: Destino::Ajuste(*a) }).collect();
        self.pick_lista("Configurações · Enter troca o valor", itens);
        if let Some(p) = self.picker.as_mut() {
            p.sel = sel.min(AJUSTES.len() - 1);
        }
    }

    /// Enter numa linha das configurações.
    pub(super) fn ajuste_aplicar(&mut self, a: Ajuste) {
        let linha = AJUSTES.iter().position(|x| *x == a).unwrap_or(0);
        match a {
            Ajuste::ArquivoConfig => return self.abrir_arquivo_de_configuracao(),
            Ajuste::Agente => return self.agente_escolher(),
            Ajuste::ModoAgente => return self.agente_modo(),
            Ajuste::Atualizar => return self.comando_astrograph("atualizar"),
            Ajuste::Saude => return self.comando_astrograph("saude"),
            Ajuste::Atalhos => return self.comando_astrograph("atalhos"),
            _ => {}
        }
        let c = crate::config::atual();
        let Some((secao, chave, valor, na_hora)) = a.trocar(&c) else { return };
        let texto = valor.to_string();
        if let Err(e) = crate::config::gravar(secao, chave, valor) {
            self.say(e, ToastKind::Error);
            self.configuracoes(linha);
            return;
        }
        let nova = crate::config::atual();
        // o que dá para aplicar agora
        match a {
            Ajuste::Tema => match nova.theme.mode.as_str() {
                "light" => crate::design::set_light(true),
                "dark" => crate::design::set_light(false),
                _ => {}
            },
            Ajuste::Quebra => self.set_ui(if nova.editor.wrap { "wrap" } else { "nowrap" }),
            Ajuste::Dicas => crate::dicas::ligar(nova.ui.tips),
            Ajuste::Formatar => self.formatar_ao_salvar = nova.lsp.format_on_save,
            Ajuste::Controle => crate::controle::ligar(nova.agentes.controle),
            _ => {}
        }
        if !na_hora {
            self.say(format!("{}: {} (vale ao reabrir o Astrograph)", a.rotulo(), a.valor(&nova)), ToastKind::Info);
        }
        let _ = texto;
        self.configuracoes(linha);
    }

    /// Abre `ui.toml` no editor; se ainda não existe, cria com o modelo comentado.
    fn abrir_arquivo_de_configuracao(&mut self) {
        let caminho = crate::config::caminho_para_gravar();
        if !caminho.is_file() {
            if let Some(d) = caminho.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Err(e) = std::fs::write(&caminho, include_str!("../../astrograph-ui.example.toml")) {
                self.say(format!("Não consegui criar {}: {e}", caminho.display()), ToastKind::Error);
                return;
            }
        }
        self.open_file(&caminho, None);
    }
}
