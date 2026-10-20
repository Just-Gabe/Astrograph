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
}

/// Teclas do editor nativo.
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct KeysCfg {
    /// Tecla líder: `"space"` (padrão), `"\\"`, `","`... Um caractere ou o nome `space`/`espaço`.
    pub leader: String,
}

impl Default for KeysCfg {
    fn default() -> Self {
        KeysCfg { leader: "space".into() }
    }
}

impl KeysCfg {
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
mod testes_teclas {
    use super::*;

    #[test]
    fn lider_aceita_nome_ou_caractere_e_cai_em_espaco() {
        let k = |s: &str| KeysCfg { leader: s.into() }.leader_char();
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
