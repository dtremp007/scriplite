use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use tempfile::Builder;

use crate::error::AppError;
use crate::model::{DiagnosticSeverity, InspectionReport};

pub fn render_human_report(report: &InspectionReport) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{}: {} pages, {:.0} x {:.0} pt\n",
        report.source.filename,
        report.source.page_count,
        report.source.width_points,
        report.source.height_points
    ));
    output.push_str(&format!(
        "Validation: {}\n",
        if report.valid { "passed" } else { "failed" }
    ));
    output.push_str(&format!("Characters: {}\n", report.characters.len()));
    for character in &report.characters {
        output.push_str(&format!(
            "  {:<24} {:>3} cues  {:>3} dual  {:>4} visual lines\n",
            character.display_name,
            character.cue_blocks,
            character.dual_blocks,
            character.highlighted_visual_lines
        ));
    }
    if !report.diagnostics.is_empty() {
        output.push_str("Diagnostics:\n");
        for diagnostic in &report.diagnostics {
            let severity = match diagnostic.severity {
                DiagnosticSeverity::Error => "error",
                DiagnosticSeverity::Warning => "warning",
            };
            let location = diagnostic
                .pdf_page
                .map(|page| format!(" page {page}"))
                .unwrap_or_default();
            let text = diagnostic
                .text
                .as_deref()
                .map(|text| format!(": {text:?}"))
                .unwrap_or_default();
            output.push_str(&format!(
                "  [{severity} {}]{location}{text} - {}\n",
                diagnostic.code, diagnostic.message
            ));
        }
    }
    output
}

pub fn write_json_atomic<T: Serialize>(value: &T, path: &Path) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| AppError::OutputIo {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut builder = Builder::new();
    // tempfile defaults to 0600; reports are ordinary output, so let the umask decide.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temporary = builder
        .tempfile_in(parent)
        .map_err(|source| AppError::OutputIo {
            path: parent.to_path_buf(),
            source,
        })?;
    serde_json::to_writer_pretty(&mut temporary, value)?;
    temporary
        .write_all(b"\n")
        .map_err(|source| AppError::OutputIo {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| AppError::OutputIo {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| AppError::OutputIo {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}
