use std::collections::BTreeMap;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, Stream};

use crate::error::AppError;
use crate::model::{Color, DialogueBlock};

pub fn write_highlighted_pdf(
    input: &Path,
    output: &Path,
    blocks: &[&DialogueBlock],
    color: Color,
) -> Result<(), AppError> {
    let mut document = Document::load(input)
        .map_err(|error| AppError::Generation(format!("could not parse source PDF: {error}")))?;
    if document.is_encrypted() {
        return Err(AppError::Generation(
            "encrypted PDFs cannot be highlighted".to_string(),
        ));
    }
    let mut rectangles: BTreeMap<usize, Vec<crate::model::Bounds>> = BTreeMap::new();
    for block in blocks {
        for run in &block.runs {
            rectangles.entry(run.page).or_default().push(run.bounds);
        }
    }
    let pages = document.get_pages();
    for (page_number, page_rectangles) in rectangles {
        let Some(page_id) = pages.get(&(page_number as u32)).copied() else {
            return Err(AppError::Generation(format!(
                "source PDF has no page {page_number}"
            )));
        };
        prepend_highlights(&mut document, page_id, &page_rectangles, color)?;
    }
    document
        .save(output)
        .map_err(|error| AppError::Generation(format!("could not save PDF: {error}")))?;
    Ok(())
}

fn prepend_highlights(
    document: &mut Document,
    page_id: lopdf::ObjectId,
    rectangles: &[crate::model::Bounds],
    color: Color,
) -> Result<(), AppError> {
    let existing = document
        .get_object(page_id)
        .map_err(|error| AppError::Generation(error.to_string()))?
        .as_dict()
        .map_err(|error| AppError::Generation(error.to_string()))?
        .get(b"Contents")
        .ok()
        .cloned();
    let content = highlight_content(rectangles, color);
    let mut stream = Stream::new(Dictionary::new(), content);
    stream
        .compress()
        .map_err(|error| AppError::Generation(error.to_string()))?;
    let highlight_id = document.add_object(stream);
    let mut contents = vec![Object::Reference(highlight_id)];
    match existing {
        Some(Object::Array(values)) => contents.extend(values),
        Some(value) => contents.push(value),
        None => {}
    }
    let page = document
        .get_object_mut(page_id)
        .map_err(|error| AppError::Generation(error.to_string()))?
        .as_dict_mut()
        .map_err(|error| AppError::Generation(error.to_string()))?;
    page.set("Contents", Object::Array(contents));
    Ok(())
}

fn highlight_content(rectangles: &[crate::model::Bounds], color: Color) -> Vec<u8> {
    let (red, green, blue) = color.components();
    let mut result = format!("q\n{red:.6} {green:.6} {blue:.6} rg\n");
    for bounds in rectangles {
        let width = bounds.width().max(0.01);
        let height = bounds.height().max(0.01);
        result.push_str(&format!(
            "{:.4} {:.4} {:.4} {:.4} re f\n",
            bounds.x0, bounds.y0, width, height
        ));
    }
    result.push_str("Q\n");
    result.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_stream_uses_exact_default_rgb() {
        let bytes = highlight_content(
            &[crate::model::Bounds {
                x0: 10.0,
                y0: 20.0,
                x1: 30.0,
                y1: 32.0,
            }],
            Color::default(),
        );
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("0.996078 0.996078 0.498039 rg"));
        assert!(text.contains("10.0000 20.0000 20.0000 12.0000 re f"));
    }
}
