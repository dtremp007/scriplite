use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppError;

pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Bounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn width(self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(self) -> f32 {
        self.y1 - self.y0
    }
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub ch: char,
    pub bounds: Bounds,
    pub baseline: f32,
    pub font_size: f32,
    pub angle_degrees: f32,
    pub source_index: usize,
}

#[derive(Debug, Clone)]
pub struct VisualRun {
    pub page: usize,
    pub baseline: f32,
    pub text: String,
    pub bounds: Bounds,
    pub font_size: f32,
    pub source_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogueLane {
    Main,
    DualLeft,
    DualRight,
}

#[derive(Debug, Clone)]
pub struct DialogueBlock {
    pub character_key: String,
    pub character_name: String,
    pub lane: DialogueLane,
    pub page: usize,
    pub cue_text: String,
    pub runs: Vec<VisualRun>,
}

#[derive(Debug, Clone)]
pub struct ExtractedPage {
    pub number: usize,
    pub width: f32,
    pub height: f32,
    pub rotation_degrees: i32,
    pub image_count: usize,
    pub glyphs: Vec<Glyph>,
    pub extracted_text: String,
}

#[derive(Debug, Clone)]
pub struct ExtractedDocument {
    pub pages: Vec<ExtractedPage>,
    pub encrypted: bool,
    pub signed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummary {
    pub filename: String,
    pub sha256: String,
    pub page_count: usize,
    pub width_points: f32,
    pub height_points: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub pdf_page: Option<usize>,
    pub printed_page: Option<String>,
    pub text: Option<String>,
    pub bounds: Option<Bounds>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(
        code: impl Into<String>,
        page: Option<usize>,
        text: Option<String>,
        bounds: Option<Bounds>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            pdf_page: page,
            printed_page: None,
            text,
            bounds,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterSummary {
    pub name: String,
    pub display_name: String,
    pub cue_blocks: usize,
    pub dual_blocks: usize,
    pub highlighted_visual_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub source: SourceSummary,
    pub valid: bool,
    pub characters: Vec<CharacterSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedFile {
    pub character: String,
    pub filename: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationSummary {
    pub color: String,
    pub requested_characters: Vec<String>,
    pub files: Vec<GeneratedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationManifest {
    pub schema_version: u32,
    pub tool_version: String,
    pub source: SourceSummary,
    pub valid: bool,
    pub characters: Vec<CharacterSummary>,
    pub diagnostics: Vec<Diagnostic>,
    pub generation: GenerationSummary,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub report: InspectionReport,
    pub blocks: Vec<DialogueBlock>,
    pub page_text: Vec<String>,
}

impl Analysis {
    pub fn blocks_by_character(&self) -> BTreeMap<&str, Vec<&DialogueBlock>> {
        let mut result: BTreeMap<&str, Vec<&DialogueBlock>> = BTreeMap::new();
        for block in &self.blocks {
            result
                .entry(block.character_key.as_str())
                .or_default()
                .push(block);
        }
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            red: 0xFE,
            green: 0xFE,
            blue: 0x7F,
        }
    }
}

impl Color {
    pub fn components(self) -> (f32, f32, f32) {
        (
            self.red as f32 / 255.0,
            self.green as f32 / 255.0,
            self.blue as f32 / 255.0,
        )
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }
}

impl FromStr for Color {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value.strip_prefix('#').unwrap_or(value);
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppError::Usage(format!(
                "invalid color '{value}'; expected #RRGGBB or RRGGBB"
            )));
        }
        let component = |range| {
            u8::from_str_radix(&hex[range], 16)
                .map_err(|_| AppError::Usage(format!("invalid color '{value}'")))
        };
        Ok(Self {
            red: component(0..2)?,
            green: component(2..4)?,
            blue: component(4..6)?,
        })
    }
}

pub fn normalize_character_name(value: &str) -> String {
    let normalized = value.nfkc().collect::<String>();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    strip_cue_extensions(&collapsed).to_uppercase()
}

pub fn strip_cue_extensions(value: &str) -> String {
    let mut result = value.trim();
    loop {
        let Some(close) = result.strip_suffix(')') else {
            break;
        };
        let Some(open_index) = close.rfind('(') else {
            break;
        };
        let extension = &close[open_index + 1..];
        let compact = extension
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>()
            .to_uppercase();
        let recognized = matches!(
            compact.as_str(),
            "CONTD" | "CONTINUED" | "VO" | "OS" | "OC" | "OFF" | "FILTERED"
        );
        if !recognized {
            break;
        }
        result = close[..open_index].trim_end();
    }
    result.to_string()
}

pub fn display_character_name(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>()
                        + &chars.flat_map(char::to_lowercase).collect::<String>()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_parser_accepts_hash_or_plain_hex() {
        assert_eq!("#FEFE7F".parse::<Color>().unwrap(), Color::default());
        assert_eq!("abcdef".parse::<Color>().unwrap().to_string(), "#ABCDEF");
        assert!("yellow".parse::<Color>().is_err());
    }

    #[test]
    fn cue_extensions_are_removed_for_matching() {
        assert_eq!(normalize_character_name(" Mike (cont'd) "), "MIKE");
        assert_eq!(normalize_character_name("RADIO HOST (V.O.)"), "RADIO HOST");
        assert_eq!(
            normalize_character_name("PEETASCH (Familje)"),
            "PEETASCH (FAMILJE)"
        );
    }
}
