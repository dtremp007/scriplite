use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use lopdf::{Document as LoDocument, Object};
use pdfium_render::prelude::*;

use crate::error::AppError;
use crate::model::{Bounds, ExtractedDocument, ExtractedPage, Glyph};

pub struct PdfEngine {
    pdfium: Pdfium,
    library_path: PathBuf,
}

/// pdfium-render binds and initializes the library once per process; later
/// engines must reuse that binding instead of loading the library again.
static BOUND_LIBRARY: OnceLock<PathBuf> = OnceLock::new();

impl PdfEngine {
    pub fn bind() -> Result<Self, AppError> {
        if let Some(library_path) = BOUND_LIBRARY.get() {
            return Ok(Self {
                pdfium: Pdfium::default(),
                library_path: library_path.clone(),
            });
        }
        let candidates = library_candidates();
        let mut failures = Vec::new();
        for candidate in candidates {
            if !candidate.is_file() {
                continue;
            }
            match Pdfium::bind_to_library(&candidate) {
                Ok(bindings) => {
                    let pdfium = Pdfium::new(bindings);
                    let _ = BOUND_LIBRARY.set(candidate.clone());
                    return Ok(Self {
                        pdfium,
                        library_path: candidate,
                    });
                }
                Err(error) => failures.push(format!("{}: {error}", candidate.display())),
            }
        }
        match Pdfium::bind_to_system_library() {
            Ok(bindings) => {
                let pdfium = Pdfium::new(bindings);
                let _ = BOUND_LIBRARY.set(PathBuf::from("<system>"));
                Ok(Self {
                    pdfium,
                    library_path: PathBuf::from("<system>"),
                })
            }
            Err(error) => {
                failures.push(format!("system library: {error}"));
                Err(AppError::PdfEngine(format!(
                    "Pdfium was not found. Place {} beside the executable or set \
                     PDFIUM_LIB_PATH. Attempts: {}",
                    Pdfium::pdfium_platform_library_name().to_string_lossy(),
                    failures.join("; ")
                )))
            }
        }
    }

    pub fn library_path(&self) -> &Path {
        &self.library_path
    }

    pub fn extract(&self, path: &Path) -> Result<ExtractedDocument, AppError> {
        let structural = LoDocument::load(path)
            .map_err(|error| AppError::InvalidPdf(format!("{}: {error}", path.display())))?;
        let encrypted = structural.is_encrypted();
        let signed = document_is_signed(&structural);
        if encrypted {
            return Ok(ExtractedDocument {
                pages: Vec::new(),
                encrypted,
                signed,
            });
        }

        let document = self
            .pdfium
            .load_pdf_from_file(path, None)
            .map_err(|error| AppError::PdfEngine(error.to_string()))?;
        let structural_pages = structural.get_pages();
        let mut pages = Vec::with_capacity(document.pages().len() as usize);
        for (page_index, page) in document.pages().iter().enumerate() {
            let width = page.width().value;
            let height = page.height().value;
            let rotation_degrees = page
                .rotation()
                .map(|rotation| rotation.as_degrees())
                .unwrap_or(0.0) as i32;
            let text = page
                .text()
                .map_err(|error| AppError::PdfEngine(error.to_string()))?;
            let extracted_text = text.all();
            let mut glyphs = Vec::new();
            for character in text.chars().iter() {
                let source_index = character.index();
                let Some(ch) = character.unicode_char() else {
                    continue;
                };
                if ch == '\0' || ch == '\r' || ch == '\n' {
                    continue;
                }
                let bounds = match character.loose_bounds() {
                    Ok(bounds) => Bounds {
                        x0: bounds.left().value,
                        y0: bounds.bottom().value,
                        x1: bounds.right().value,
                        y1: bounds.top().value,
                    },
                    Err(_) if ch.is_whitespace() => continue,
                    Err(error) => return Err(AppError::PdfEngine(error.to_string())),
                };
                let baseline = character
                    .origin_y()
                    .map(|points| points.value)
                    .unwrap_or(bounds.y0);
                glyphs.push(Glyph {
                    ch,
                    bounds,
                    baseline,
                    font_size: character.scaled_font_size().value,
                    angle_degrees: character.angle_degrees().unwrap_or(0.0),
                    source_index,
                });
            }
            pages.push(ExtractedPage {
                number: page_index + 1,
                width,
                height,
                rotation_degrees,
                image_count: structural_pages
                    .get(&((page_index + 1) as u32))
                    .and_then(|page_id| structural.get_page_images(*page_id).ok())
                    .map(|images| images.len())
                    .unwrap_or(0),
                glyphs,
                extracted_text,
            });
        }
        Ok(ExtractedDocument {
            pages,
            encrypted,
            signed,
        })
    }
}

fn library_candidates() -> Vec<PathBuf> {
    let library_name = Pdfium::pdfium_platform_library_name();
    let mut result = Vec::new();
    if let Some(path) = env::var_os("PDFIUM_LIB_PATH") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            result.push(path.join(&library_name));
        } else {
            result.push(path);
        }
    }
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        result.push(parent.join(&library_name));
    }
    if let Ok(current) = env::current_dir() {
        result.push(current.join(&library_name));
        if let Some(platform) = vendored_platform() {
            result.push(
                current
                    .join("vendor/pdfium")
                    .join(platform)
                    .join("lib")
                    .join(&library_name),
            );
        }
    }
    result
}

/// The `scripts/fetch_pdfium.sh` directory name for the running platform.
fn vendored_platform() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        _ => None,
    }
}

fn document_is_signed(document: &LoDocument) -> bool {
    document.objects.values().any(|object| {
        let Ok(dictionary) = object.as_dict() else {
            return false;
        };
        matches!(
            dictionary.get(b"FT"),
            Ok(Object::Name(name)) if name.as_slice() == b"Sig"
        )
    })
}
