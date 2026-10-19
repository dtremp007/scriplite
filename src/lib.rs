pub mod analyze;
pub mod batch;
pub mod engine;
pub mod error;
pub mod model;
pub mod pdf_writer;
pub mod report;

pub use analyze::analyze_pdf;
pub use batch::{GenerateOptions, generate};
pub use engine::PdfEngine;
pub use error::{AppError, ExitCode};
pub use model::{Analysis, Color, InspectionReport};
