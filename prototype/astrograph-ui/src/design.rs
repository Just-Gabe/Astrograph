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

/// Cor, negrito, itálico de cada classe.
pub fn syntax(c: Cls) -> (u32, bool, bool) {
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

pub const BG: u32 = 0x001e_1e22;
pub const SURFACE: u32 = 0x002c_2c31;
pub const SURFACE_HI: u32 = 0x0036_3640;
pub const TEXT: u32 = 0x00df_dfe0;
pub const TEXT_STRONG: u32 = 0x00f5_f5f7;
pub const DIM: u32 = 0x008e_8e96;
pub const FAINT: u32 = 0x0058_5860;
pub const ACCENT: u32 = 0x000a_84ff;
pub const GREEN: u32 = 0x0030_d158;
pub const ORANGE: u32 = 0x00ff_9f0a;
pub const PURPLE: u32 = 0x00bf_5af2;
pub const RED: u32 = 0x00ff_453a;
pub const WHITE: u32 = 0x00ff_ffff;
pub const BLACK: u32 = 0x0000_0000;

/// Modo mostrado na pílula de status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeKind {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
    Search,
}

impl ModeKind {
    pub fn label(self) -> &'static str {
        match self {
            ModeKind::Normal => "Normal",
            ModeKind::Insert => "Inserir",
            ModeKind::Visual => "Visual",
            ModeKind::VisualLine => "Linhas",
            ModeKind::Command => "Comando",
            ModeKind::Search => "Buscar",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            ModeKind::Normal => ACCENT,
            ModeKind::Insert => GREEN,
            ModeKind::Visual | ModeKind::VisualLine => PURPLE,
            ModeKind::Command | ModeKind::Search => ORANGE,
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
