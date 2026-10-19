use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    Usage = 2,
    Input = 3,
    Validation = 4,
    Collision = 5,
    Generation = 6,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Usage(String),
    #[error("could not read input PDF '{path}': {source}")]
    InputIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("PDF engine error: {0}")]
    PdfEngine(String),
    #[error("invalid or unsupported PDF: {0}")]
    InvalidPdf(String),
    #[error("screenplay validation failed")]
    Validation,
    #[error("unknown character '{0}'")]
    UnknownCharacter(String),
    #[error("destination already exists: {0}")]
    Collision(PathBuf),
    #[error("generation failed: {0}")]
    Generation(String),
    #[error("could not write '{path}': {source}")]
    OutputIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

impl AppError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::Usage,
            Self::InputIo { .. } | Self::PdfEngine(_) | Self::InvalidPdf(_) => ExitCode::Input,
            Self::Validation | Self::UnknownCharacter(_) => ExitCode::Validation,
            Self::Collision(_) => ExitCode::Collision,
            Self::Generation(_) | Self::OutputIo { .. } | Self::Json(_) => ExitCode::Generation,
        }
    }
}
