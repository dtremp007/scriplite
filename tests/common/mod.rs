#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use lopdf::Document;
use scriplite::PdfEngine;

/// Pdfium is not thread-safe, so tests that use it run one at a time.
static PDFIUM_TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    PDFIUM_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Binds Pdfium, or returns None so the test can skip. Panics instead when
/// SCRIPLITE_REQUIRE_PDFIUM is set, so CI cannot silently skip.
pub fn bind_engine() -> Option<PdfEngine> {
    match PdfEngine::bind() {
        Ok(engine) => Some(engine),
        Err(error) if std::env::var_os("SCRIPLITE_REQUIRE_PDFIUM").is_some() => {
            panic!("SCRIPLITE_REQUIRE_PDFIUM is set but Pdfium is unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skipping PDF test because Pdfium is unavailable: {error}");
            None
        }
    }
}

/// Counts filled rectangles in content streams that set `fill_operator`,
/// for example `0.996078 0.996078 0.498039 rg`.
pub fn count_highlight_rectangles(path: &Path, fill_operator: &str) -> usize {
    let document = Document::load(path).unwrap();
    let mut count = 0;
    for page_id in document.get_pages().into_values() {
        for content_id in document.get_page_contents(page_id) {
            let Ok(stream) = document
                .get_object(content_id)
                .and_then(|object| object.as_stream())
            else {
                continue;
            };
            let Ok(bytes) = stream.decompressed_content() else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            if text.contains(fill_operator) {
                count += text.matches(" re f\n").count();
            }
        }
    }
    count
}
