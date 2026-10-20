//! Design do editor nativo: cores, medidas e classes de realce.
//! Escuro, sóbrio e macio (inspirado nos temas escuros da Apple): superfícies em
//! camadas, um acento azul, cantos arredondados e muito espaço.

/// Classes de realce a partir do nome da captura do tree-sitter.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cls {
    Comment,
    Str,
    Number,
    Keyword,
    Function,
    Type,
    Operator,
    Punct,
    Property,
    Variable,
    Tag,
}

pub fn classify(group: &str) -> Option<Cls> {
    let g = group.trim_start_matches('@');
    let head = g.split('.').next().unwrap_or(g);
    Some(match head {
        "comment" => Cls::Comment,
        "string" | "character" => Cls::Str,
        "number" | "boolean" | "constant" | "float" => Cls::Number,
        "keyword" | "conditional" | "repeat" | "include" | "exception" | "label" | "storageclass" => Cls::Keyword,
        "function" | "method" | "constructor" => Cls::Function,
        "type" | "namespace" | "module" | "attribute" => Cls::Type,
        "operator" => Cls::Operator,
        "punctuation" | "delimiter" => Cls::Punct,
        "property" | "field" | "parameter" => Cls::Property,
        "variable" => {
            if g.contains("builtin") {
                Cls::Keyword
            } else if g.contains("member") || g.contains("parameter") {
                Cls::Property
            } else {
                return None;
            }
        }
        "tag" => Cls::Tag,
        _ => return None,
    })
}

use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

/// Liga o tema claro (`true`) ou o escuro. Vale para a cena nativa inteira.
pub fn set_light(light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
}

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

fn pick(dark: u32, light: u32) -> u32 {
    if is_light() { light } else { dark }
}

/// Cor, negrito, itálico de cada classe.
pub fn syntax(c: Cls) -> (u32, bool, bool) {
    if is_light() {
        // claro: as cores do Xcode Light
        return match c {
            Cls::Comment => (0x005d_6c79, false, true),
            Cls::Str => (0x00c4_1a16, false, false),
            Cls::Number => (0x001c_00cf, false, false),
            Cls::Keyword => (0x00aa_0d91, true, false),
            Cls::Function => (0x0032_6d74, false, false),
            Cls::Type => (0x000b_4f79, false, false),
            Cls::Operator => (0x003f_3f44, false, false),
            Cls::Punct => (0x005a_5a60, false, false),
            Cls::Property => (0x0064_3820, false, false),
            Cls::Variable => (0x001d_1d1f, false, false),
            Cls::Tag => (0x00aa_0d91, false, false),
        };
    }
    match c {
        Cls::Comment => (0x006c_7986, false, true),
        Cls::Str => (0x00fc_6a5d, false, false),
        Cls::Number => (0x00d0_bf69, false, false),
        Cls::Keyword => (0x00fc_5fa3, true, false),
        Cls::Function => (0x0067_b7a4, false, false),
        Cls::Type => (0x005d_d8ff, false, false),
        Cls::Operator => (0x00b0_b0b8, false, false),
        Cls::Punct => (0x008a_8a94, false, false),
        Cls::Property => (0x00a1_67e6, false, false),
        Cls::Variable => (0x00df_dfe0, false, false),
        Cls::Tag => (0x00fc_5fa3, false, false),
    }
}

/// Quanto das sombras e do véu escuro vale no tema atual (no claro, bem mais leve).
pub fn k_sombra() -> f32 {
    if is_light() { 0.32 } else { 1.0 }
}

pub fn bg() -> u32 {
    pick(0x001e_1e22, 0x00f7_f7f9)
}
pub fn surface() -> u32 {
    pick(0x002c_2c31, 0x00ec_eef2)
}
pub fn surface_hi() -> u32 {
    pick(0x0036_3640, 0x00ff_ffff)
}
pub fn text() -> u32 {
    pick(0x00df_dfe0, 0x001d_1d1f)
}
pub fn text_strong() -> u32 {
    pick(0x00f5_f5f7, 0x0000_0000)
}
pub fn dim() -> u32 {
    pick(0x008e_8e96, 0x006e_6e73)
}
pub fn faint() -> u32 {
    pick(0x0058_5860, 0x00a8_a8b0)
}
/// acento para texto destacado (claro sobre fundo escuro, mais fechado no claro)
pub fn accent_soft() -> u32 {
    pick(0x0064_b5ff, 0x0000_5fd6)
}
/// cor das camadas translúcidas (bordas, realces): branco no escuro, preto no claro
pub fn white() -> u32 {
    pick(0x00ff_ffff, 0x0000_0000)
}
pub const ACCENT: u32 = 0x000a_84ff;
pub const GREEN: u32 = 0x0030_d158;
pub const ORANGE: u32 = 0x00ff_9f0a;
pub const PURPLE: u32 = 0x00bf_5af2;
pub const RED: u32 = 0x00ff_453a;
pub const BLACK: u32 = 0x0000_0000;

/// Modo mostrado na pílula de status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeKind {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
    Command,
    Search,
    Terminal,
}

impl ModeKind {
    pub fn label(self) -> &'static str {
        match self {
            ModeKind::Normal => "Normal",
            ModeKind::Insert => "Inserir",
            ModeKind::Visual => "Visual",
            ModeKind::VisualLine => "Linhas",
            ModeKind::VisualBlock => "Bloco",
            ModeKind::Command => "Comando",
            ModeKind::Search => "Buscar",
            ModeKind::Terminal => "Terminal",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            ModeKind::Normal => ACCENT,
            ModeKind::Insert => GREEN,
            ModeKind::Visual | ModeKind::VisualLine | ModeKind::VisualBlock => PURPLE,
            ModeKind::Command | ModeKind::Search => ORANGE,
            ModeKind::Terminal => GREEN,
        }
    }
}

/// Medidas em pixels lógicos (multiplicadas pela escala da tela).
pub struct Metrics {
    pub pad_x: f32,
    pub title_h: f32,
    pub bottom: f32,
    pub radius: f32,
    pub line_height: f32,
}

pub const M: Metrics = Metrics { pad_x: 26.0, title_h: 46.0, bottom: 70.0, radius: 10.0, line_height: 1.62 };
