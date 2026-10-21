//! Configuracao da UI (arquivo TOML opcional).

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub window: WindowCfg,
    pub font: FontCfg,
    pub cursor: CursorCfg,
    pub scroll: ScrollCfg,
    pub keys: KeysCfg,
    pub clock: ClockCfg,
    pub engine: EngineCfg,
    pub lsp: LspCfg,
    pub editor: EditorCfg,
    pub theme: ThemeCfg,
}

/// Tema do app nativo.
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ThemeCfg {
    /// `"dark"` (padrão), `"light"` ou `"auto"` (segue o tema do sistema). `:set background=light|dark` troca na hora.
    pub mode: String,
}

impl Default for ThemeCfg {
    fn default() -> Self {
        ThemeCfg { mode: "dark".into() }
    }
}

/// Opções do editor nativo (as de `:set`).
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct EditorCfg {
    /// `"unnamedplus"`: copiar e colar sem registrador usa o clipboard do sistema (`:set clipboard=unnamedplus`)
    pub clipboard: String,
    /// `"manual"` (padrão), `"indent"` ou `"syntax"` (tree-sitter): como as dobras (`za`, `zM`...) são achadas
    pub foldmethod: String,
    /// quebra as linhas longas na largura da tela (`:set wrap`)
    pub wrap: bool,
    /// com `wrap`, quebra nos espaços (a palavra inteira desce) em vez de partir no meio
    pub linebreak: bool,
}

impl Default for EditorCfg {
    fn default() -> Self {
        EditorCfg { clipboard: String::new(), foldmethod: "manual".into(), wrap: false, linebreak: false }
    }
}

/// Servidores de linguagem do motor nativo.
#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct LspCfg {
    /// pede ao servidor para formatar o arquivo antes de gravar (`:w`)
    pub format_on_save: bool,
}

/// Qual motor edita: o nativo em Rust (padrão) ou o Neovim.
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct EngineCfg {
    /// `true` (padrão) abre no motor nativo, sem Neovim; `false` abre o Neovim. `ASTROGRAPH_ENGINE=nvim|native` manda sobre isto.
    pub native: bool,
}

impl Default for EngineCfg {
    fn default() -> Self {
        EngineCfg { native: true }
    }
}

/// Relógio pequeno no canto inferior direito.
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ClockCfg {
    pub enabled: bool,
    /// `true` mostra `3:07 PM`; `false` (padrão) mostra `15:07`
    pub hour12: bool,
}

impl Default for ClockCfg {
    fn default() -> Self {
        ClockCfg { enabled: true, hour12: false }
    }
}

/// Teclas do editor nativo.
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct KeysCfg {
    /// Tecla líder: `"space"` (padrão), `"\\"`, `","`... Um caractere ou o nome `space`/`espaço`.
    pub leader: String,
    /// mapeamentos (sem remapeamento) por modo: `"<Leader>w" = ":w<CR>"`
    pub normal: std::collections::BTreeMap<String, String>,
    pub insert: std::collections::BTreeMap<String, String>,
    pub visual: std::collections::BTreeMap<String, String>,
}

impl Default for KeysCfg {
    fn default() -> Self {
        KeysCfg { leader: "space".into(), normal: Default::default(), insert: Default::default(), visual: Default::default() }
    }
}

impl KeysCfg {
    /// Os mapeamentos do arquivo como (modos, lhs, rhs): modos são os bits de `astrograph_core::mapas`.
    pub fn mapeamentos(&self) -> Vec<(u8, String, String)> {
        use astrograph_core::mapas::{MODO_I, MODO_N, MODO_V};
        let mut v = Vec::new();
        for (modo, tabela) in [(MODO_N, &self.normal), (MODO_I, &self.insert), (MODO_V, &self.visual)] {
            for (l, r) in tabela {
                v.push((modo, l.clone(), r.clone()));
            }
        }
        v
    }

    /// A líder como caractere (`space`, `<Space>`, `espaço` ou um único caractere); inválida volta a Espaço.
    pub fn leader_char(&self) -> char {
        let t = self.leader.trim();
        let low = t.trim_matches(['<', '>']).to_lowercase();
        if matches!(low.as_str(), "space" | "espaço" | "espaco" | "") {
            return ' ';
        }
        let mut it = t.chars();
        match (it.next(), it.next()) {
            (Some(c), None) if !c.is_control() => c,
            _ => ' ',
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct WindowCfg {
    pub width: f64,
    pub height: f64,
    pub title: String,
    /// Margem (pixels logicos) entre a borda da janela e o texto.
    pub padding: f64,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct FontCfg {
    pub size: f32,
    pub line_height: f32,
    /// Caminho ou nome de arquivo de fonte (procurado em C:\Windows\Fonts).
    pub regular: Option<String>,
    pub bold: Option<String>,
    pub italic: Option<String>,
    /// Fontes de fallback, tentadas por glifo quando a principal nao tem o
    /// caractere (emoji, simbolos, CJK). Ordem importa.
    pub fallback: Vec<String>,
    /// Ligaduras de programacao (`->`, `=>`, `!=`, `===`) quando a fonte tem.
    pub ligatures: bool,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct CursorCfg {
    pub blink: bool,
    pub blink_ms: u64,
    /// Duracao do deslize do cursor entre posicoes (0 desliga).
    pub animation_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window: WindowCfg::default(),
            font: FontCfg::default(),
            cursor: CursorCfg::default(),
            scroll: ScrollCfg::default(),
            keys: KeysCfg::default(),
            clock: ClockCfg::default(),
            engine: EngineCfg::default(),
            lsp: LspCfg::default(),
            editor: EditorCfg::default(),
            theme: ThemeCfg::default(),
        }
    }
}

impl Default for WindowCfg {
    fn default() -> Self {
        WindowCfg {
            width: 1280.0,
            height: 800.0,
            title: "Astrograph".to_string(),
            padding: 10.0,
        }
    }
}

impl Default for FontCfg {
    fn default() -> Self {
        FontCfg {
            size: 18.0,
            line_height: 1.2,
            regular: None,
            bold: None,
            italic: None,
            fallback: vec![
                "seguiemj.ttf".to_string(),
                "seguisym.ttf".to_string(),
                "malgun.ttf".to_string(),
            ],
            ligatures: true,
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ScrollCfg {
    /// Duracao da rolagem suave por pixel (0 desliga).
    pub animation_ms: u64,
}

impl Default for ScrollCfg {
    fn default() -> Self {
        ScrollCfg { animation_ms: 160 }
    }
}

impl Default for CursorCfg {
    fn default() -> Self {
        CursorCfg {
            blink: true,
            blink_ms: 530,
            animation_ms: 80,
        }
    }
}

impl Config {
    /// Carrega a config do primeiro caminho existente; senao usa os defaults.
    pub fn load() -> (Config, Option<PathBuf>) {
        for path in candidate_paths() {
            if path.is_file() {
                match std::fs::read_to_string(&path) {
                    Ok(text) => match toml::from_str::<Config>(&text) {
                        Ok(config) => return (config, Some(path)),
                        Err(err) => {
                            eprintln!("ASTROGRAPH_UI: config invalida em {:?}: {}", path, err);
                        }
                    },
                    Err(err) => eprintln!("ASTROGRAPH_UI: nao li {:?}: {}", path, err),
                }
            }
        }
        (Config::default(), None)
    }
}

/// O arquivo de config que está valendo (o primeiro que existe), se houver.
pub fn caminho_config() -> Option<PathBuf> {
    candidate_paths().into_iter().find(|p| p.is_file())
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(arg) = cli_config() {
        paths.push(PathBuf::from(arg));
    }
    if let Ok(env) = std::env::var("ASTROGRAPH_UI_CONFIG") {
        paths.push(PathBuf::from(env));
    }
    paths.push(PathBuf::from("astrograph-ui.toml"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("astrograph-ui.toml"));
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        paths.push(Path::new(&appdata).join("astrograph").join("ui.toml"));
    }
    paths
}

fn cli_config() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next();
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod testes_mapas {
    use super::*;

    #[test]
    fn keymaps_do_toml_viram_mapeamentos_por_modo() {
        let c: Config = toml::from_str("[keys]\nleader = \",\"\n[keys.normal]\n\"<Leader>w\" = \":w<CR>\"\n[keys.insert]\n\"jk\" = \"<Esc>\"\n[keys.visual]\n\"<Leader>y\" = \"\\\"+y\"").unwrap();
        let m = c.keys.mapeamentos();
        assert_eq!(m.len(), 3);
        assert!(m.contains(&(astrograph_core::mapas::MODO_N, "<Leader>w".into(), ":w<CR>".into())));
        assert!(m.contains(&(astrograph_core::mapas::MODO_I, "jk".into(), "<Esc>".into())));
        assert!(m.contains(&(astrograph_core::mapas::MODO_V, "<Leader>y".into(), "\"+y".into())));
        assert_eq!(c.keys.leader_char(), ',');
        // sem a seção: nada
        assert!(Config::default().keys.mapeamentos().is_empty());
    }
}

#[cfg(test)]
mod testes_teclas {
    use super::*;

    #[test]
    fn lider_aceita_nome_ou_caractere_e_cai_em_espaco() {
        let k = |s: &str| KeysCfg { leader: s.into(), ..Default::default() }.leader_char();
        assert_eq!(k("space"), ' ');
        assert_eq!(k("<Space>"), ' ');
        assert_eq!(k("Espaço"), ' ');
        assert_eq!(k(","), ',');
        assert_eq!(k("\\"), '\\');
        assert_eq!(k("ab"), ' ');
        assert_eq!(k(""), ' ');
        let c: Config = toml::from_str("[keys]\nleader = \";\"").unwrap();
        assert_eq!(c.keys.leader_char(), ';');
    }
}

#[cfg(test)]
mod testes_motor {
    use super::*;

    #[test]
    fn o_padrao_e_o_motor_nativo_e_o_tema_escuro() {
        assert!(Config::default().engine.native);
        assert!(toml::from_str::<Config>("").unwrap().engine.native);
        assert!(toml::from_str::<Config>("[engine]").unwrap().engine.native, "tabela sem a chave");
        assert!(!toml::from_str::<Config>("[engine]\nnative = false").unwrap().engine.native, "o Neovim ainda é uma escolha");
        assert_eq!(Config::default().theme.mode, "dark");
    }
}
