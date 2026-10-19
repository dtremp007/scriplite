use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::engine::PdfEngine;
use crate::error::AppError;
use crate::model::{
    Analysis, Bounds, CharacterSummary, Diagnostic, DiagnosticSeverity, DialogueBlock,
    DialogueLane, ExtractedDocument, ExtractedPage, InspectionReport, REPORT_SCHEMA_VERSION,
    SourceSummary, VisualRun, display_character_name, normalize_character_name,
    strip_cue_extensions,
};

const BASELINE_TOLERANCE_POINTS: f32 = 1.5;
const PAGE_GEOMETRY_TOLERANCE_POINTS: f32 = 0.25;
const ANGLE_TOLERANCE_DEGREES: f32 = 0.5;

#[derive(Debug)]
struct Row {
    runs: Vec<VisualRun>,
}

#[derive(Debug, Clone)]
struct Cue {
    row_index: usize,
    run_index: usize,
    lane: DialogueLane,
}

pub fn analyze_pdf(engine: &PdfEngine, path: &Path) -> Result<Analysis, AppError> {
    let bytes = fs::read(path).map_err(|source| AppError::InputIo {
        path: path.to_path_buf(),
        source,
    })?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let document = engine.extract(path)?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("input.pdf")
        .to_string();
    Ok(analyze_extracted(document, filename, sha256))
}

pub fn analyze_extracted(
    document: ExtractedDocument,
    filename: String,
    sha256: String,
) -> Analysis {
    let mut diagnostics = validate_document_shell(&document);
    let width = document.pages.first().map(|page| page.width).unwrap_or(0.0);
    let height = document
        .pages
        .first()
        .map(|page| page.height)
        .unwrap_or(0.0);
    let mut blocks = Vec::new();
    let mut all_runs = Vec::new();
    let mut claimed: BTreeSet<(usize, i32, i32)> = BTreeSet::new();

    for page in &document.pages {
        let rows = cluster_page(page);
        let (page_blocks, page_diagnostics) = parse_page(page, &rows);
        for block in &page_blocks {
            for run in &block.runs {
                claimed.insert(run_identity(run));
            }
        }
        diagnostics.extend(page_diagnostics);
        blocks.extend(page_blocks);
        all_runs.extend(rows.into_iter().flat_map(|row| row.runs));
    }

    validate_unclaimed_dialogue(&document, &all_runs, &claimed, &mut diagnostics);
    validate_block_overlaps(&blocks, &mut diagnostics);
    attach_printed_pages(&document, &all_runs, &mut diagnostics);

    let mut character_order = Vec::<String>::new();
    let mut character_names = BTreeMap::<String, String>::new();
    for block in &blocks {
        if !character_names.contains_key(&block.character_key) {
            character_order.push(block.character_key.clone());
            character_names.insert(block.character_key.clone(), block.character_name.clone());
        }
    }
    let mut characters = Vec::new();
    for key in character_order {
        let matching = blocks
            .iter()
            .filter(|block| block.character_key == key)
            .collect::<Vec<_>>();
        let name = character_names[&key].clone();
        characters.push(CharacterSummary {
            display_name: display_character_name(&name),
            name,
            cue_blocks: matching.len(),
            dual_blocks: matching
                .iter()
                .filter(|block| block.lane != DialogueLane::Main)
                .count(),
            highlighted_visual_lines: matching.iter().map(|block| block.runs.len()).sum(),
        });
    }

    let valid = !diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Error);
    let report = InspectionReport {
        schema_version: REPORT_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SourceSummary {
            filename,
            sha256,
            page_count: document.pages.len(),
            width_points: width,
            height_points: height,
        },
        valid,
        characters,
        diagnostics,
    };
    Analysis {
        report,
        blocks,
        page_text: document
            .pages
            .iter()
            .map(|page| page.extracted_text.clone())
            .collect(),
    }
}

fn validate_document_shell(document: &ExtractedDocument) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if document.encrypted {
        diagnostics.push(Diagnostic::error(
            "E_ENCRYPTED_PDF",
            None,
            None,
            None,
            "encrypted or password-protected PDFs are not supported",
        ));
        return diagnostics;
    }
    if document.signed {
        diagnostics.push(Diagnostic::error(
            "E_SIGNED_PDF",
            None,
            None,
            None,
            "editing would invalidate the PDF's digital signature",
        ));
    }
    if document.pages.is_empty() {
        diagnostics.push(Diagnostic::error(
            "E_NO_PAGES",
            None,
            None,
            None,
            "the PDF contains no pages",
        ));
        return diagnostics;
    }
    let reference = &document.pages[0];
    let total_glyphs = document
        .pages
        .iter()
        .map(|page| page.glyphs.len())
        .sum::<usize>();
    if total_glyphs == 0 {
        diagnostics.push(Diagnostic::error(
            "E_NO_TEXT_LAYER",
            None,
            None,
            None,
            "no positioned text glyphs were found; scanned PDFs require OCR and are unsupported",
        ));
    }
    for page in &document.pages {
        if page.rotation_degrees != 0 {
            diagnostics.push(Diagnostic::error(
                "E_PAGE_ROTATION",
                Some(page.number),
                None,
                None,
                format!("page rotation is {} degrees", page.rotation_degrees),
            ));
        }
        if (page.width - reference.width).abs() > PAGE_GEOMETRY_TOLERANCE_POINTS
            || (page.height - reference.height).abs() > PAGE_GEOMETRY_TOLERANCE_POINTS
        {
            diagnostics.push(Diagnostic::error(
                "E_PAGE_GEOMETRY",
                Some(page.number),
                None,
                None,
                format!(
                    "page is {:.2} x {:.2} pt; expected {:.2} x {:.2} pt",
                    page.width, page.height, reference.width, reference.height
                ),
            ));
        }
        if page.extracted_text.contains('\u{fffd}') {
            diagnostics.push(Diagnostic::error(
                "E_TEXT_DECODE",
                Some(page.number),
                None,
                None,
                "the text layer contains undecodable replacement characters",
            ));
        }
        if page.glyphs.is_empty() && page.image_count > 0 {
            diagnostics.push(Diagnostic::error(
                "E_SCANNED_PAGE",
                Some(page.number),
                None,
                None,
                "the page contains image content but no positioned text glyphs",
            ));
        }
        if let Some(glyph) = page
            .glyphs
            .iter()
            .find(|glyph| glyph.angle_degrees.abs() > ANGLE_TOLERANCE_DEGREES)
        {
            diagnostics.push(Diagnostic::error(
                "E_GLYPH_ROTATION",
                Some(page.number),
                Some(glyph.ch.to_string()),
                Some(glyph.bounds),
                format!(
                    "rotated text ({:.2} degrees) is unsupported",
                    glyph.angle_degrees
                ),
            ));
        }
    }
    diagnostics
}

fn cluster_page(page: &ExtractedPage) -> Vec<Row> {
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| !glyph.ch.is_whitespace() && glyph.bounds.width() > 0.01)
        .collect::<Vec<_>>();
    glyphs.sort_by(|left, right| {
        right
            .baseline
            .partial_cmp(&left.baseline)
            .unwrap_or(Ordering::Equal)
            .then(left.source_index.cmp(&right.source_index))
    });
    let mut groups: Vec<Vec<&crate::model::Glyph>> = Vec::new();
    for glyph in glyphs {
        if let Some(group) = groups.last_mut() {
            let mean = group.iter().map(|item| item.baseline).sum::<f32>() / group.len() as f32;
            if (glyph.baseline - mean).abs() <= BASELINE_TOLERANCE_POINTS {
                group.push(glyph);
                continue;
            }
        }
        groups.push(vec![glyph]);
    }

    groups
        .into_iter()
        .map(|mut group| {
            group.sort_by(|left, right| {
                left.bounds
                    .x0
                    .partial_cmp(&right.bounds.x0)
                    .unwrap_or(Ordering::Equal)
            });
            let baseline = group.iter().map(|item| item.baseline).sum::<f32>() / group.len() as f32;
            let mut widths = group
                .iter()
                .map(|item| item.bounds.width())
                .filter(|value| *value > 0.5)
                .collect::<Vec<_>>();
            widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let advance = widths
                .get(widths.len() / 2)
                .copied()
                .unwrap_or_else(|| group[0].font_size * 0.6)
                .max(1.0);
            let split_gap = (advance * 8.0).max(36.0);
            let mut chunks: Vec<Vec<&crate::model::Glyph>> = vec![Vec::new()];
            for glyph in group {
                let split = chunks
                    .last()
                    .and_then(|chunk| chunk.last())
                    .is_some_and(|previous| glyph.bounds.x0 - previous.bounds.x1 > split_gap);
                if split {
                    chunks.push(Vec::new());
                }
                chunks.last_mut().unwrap().push(glyph);
            }
            let runs = chunks
                .into_iter()
                .filter(|chunk| !chunk.is_empty())
                .map(|chunk| build_run(page.number, baseline, chunk, advance))
                .collect();
            Row { runs }
        })
        .collect()
}

fn build_run(
    page: usize,
    baseline: f32,
    glyphs: Vec<&crate::model::Glyph>,
    advance: f32,
) -> VisualRun {
    let mut text = String::new();
    let mut bounds = glyphs[0].bounds;
    let mut previous: Option<f32> = None;
    for glyph in &glyphs {
        if let Some(previous_x1) = previous {
            let gap = glyph.bounds.x0 - previous_x1;
            if gap > advance * 0.35 {
                let spaces = (gap / advance).round().clamp(1.0, 8.0) as usize;
                text.extend(std::iter::repeat_n(' ', spaces));
            }
        }
        text.push(glyph.ch);
        bounds = bounds.union(glyph.bounds);
        previous = Some(glyph.bounds.x1);
    }
    let font_size = glyphs.iter().map(|glyph| glyph.font_size).sum::<f32>() / glyphs.len() as f32;
    VisualRun {
        page,
        baseline,
        text: text.trim().to_string(),
        bounds,
        font_size,
        source_index: glyphs
            .iter()
            .map(|glyph| glyph.source_index)
            .min()
            .unwrap_or(0),
    }
}

fn parse_page(page: &ExtractedPage, rows: &[Row]) -> (Vec<DialogueBlock>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut cues = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        let left = row.runs.iter().enumerate().find(|(_, run)| {
            cue_text_shape(&run.text)
                && lane_for_x(run.bounds.x0, page.width) == Some(DialogueLane::DualLeft)
        });
        let right = row.runs.iter().enumerate().find(|(_, run)| {
            cue_text_shape(&run.text)
                && lane_for_x(run.bounds.x0, page.width) == Some(DialogueLane::DualRight)
        });
        if let (Some((left_index, _)), Some((right_index, _))) = (left, right) {
            if has_following_content(rows, row_index, DialogueLane::DualLeft, page.width)
                && has_following_content(rows, row_index, DialogueLane::DualRight, page.width)
            {
                cues.push(Cue {
                    row_index,
                    run_index: left_index,
                    lane: DialogueLane::DualLeft,
                });
                cues.push(Cue {
                    row_index,
                    run_index: right_index,
                    lane: DialogueLane::DualRight,
                });
            } else {
                diagnostics.push(Diagnostic::error(
                    "E_MALFORMED_DUAL_DIALOGUE",
                    Some(page.number),
                    Some(
                        row.runs
                            .iter()
                            .map(|run| run.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" | "),
                    ),
                    Some(row.runs[0].bounds),
                    "paired dual-dialogue cues do not each have lane-local dialogue",
                ));
            }
        } else if left.is_some() || right.is_some() {
            let (run_index, run) = left.or(right).unwrap();
            let lane = lane_for_x(run.bounds.x0, page.width).unwrap();
            if has_following_content(rows, row_index, lane, page.width) {
                diagnostics.push(Diagnostic::error(
                    "E_MALFORMED_DUAL_DIALOGUE",
                    Some(page.number),
                    Some(run.text.clone()),
                    Some(run.bounds),
                    "a dual-dialogue cue has no matching cue in the opposite lane",
                ));
            }
            let _ = run_index;
        }

        for (run_index, run) in row.runs.iter().enumerate() {
            if lane_for_x(run.bounds.x0, page.width) != Some(DialogueLane::Main)
                || !cue_text_shape(&run.text)
                || is_continuation_marker(&run.text)
            {
                continue;
            }
            if contains_group_separator(&run.text) {
                diagnostics.push(Diagnostic::error(
                    "E_AMBIGUOUS_CUE",
                    Some(page.number),
                    Some(run.text.clone()),
                    Some(run.bounds),
                    "combined or group cue separators are unsupported",
                ));
            } else if has_following_content(rows, row_index, DialogueLane::Main, page.width) {
                cues.push(Cue {
                    row_index,
                    run_index,
                    lane: DialogueLane::Main,
                });
            } else if page.number > 1 {
                diagnostics.push(Diagnostic::error(
                    "E_CUE_LIKE_ACTION",
                    Some(page.number),
                    Some(run.text.clone()),
                    Some(run.bounds),
                    "cue-lane uppercase text is not followed by dialogue",
                ));
            }
        }
    }
    cues.sort_by_key(|cue| (cue.row_index, cue.run_index));

    let mut blocks = Vec::new();
    for cue in &cues {
        let cue_run = rows[cue.row_index].runs[cue.run_index].clone();
        let character_name = strip_cue_extensions(&cue_run.text);
        let character_key = normalize_character_name(&character_name);
        if character_key.is_empty() {
            continue;
        }
        let mut runs = vec![cue_run.clone()];
        for (row_index, row) in rows.iter().enumerate().skip(cue.row_index + 1) {
            if cues.iter().any(|other| other.row_index == row_index) {
                break;
            }
            let matching = row
                .runs
                .iter()
                .find(|run| content_for_lane(run, cue.lane, page.width));
            if let Some(run) = matching {
                runs.push(run.clone());
                continue;
            }
            if let Some(marker) = row
                .runs
                .iter()
                .find(|run| is_continuation_marker(&run.text))
            {
                runs.push(marker.clone());
                continue;
            }
            if row.runs.iter().any(|run| action_like(run, page.width)) {
                break;
            }
            if !row.runs.is_empty() {
                break;
            }
        }
        if runs.len() > 1 {
            blocks.push(DialogueBlock {
                character_key,
                character_name,
                lane: cue.lane,
                page: page.number,
                cue_text: cue_run.text,
                runs,
            });
        }
    }
    (blocks, diagnostics)
}

fn cue_text_shape(text: &str) -> bool {
    let base = strip_cue_extensions(text);
    let has_letter = base.chars().any(char::is_alphabetic);
    let uppercase = base
        .chars()
        .filter(|ch| ch.is_alphabetic())
        .all(|ch| ch.is_uppercase());
    has_letter && uppercase && base.chars().count() <= 48
}

fn contains_group_separator(text: &str) -> bool {
    text.contains('/') || text.contains('&') || text.contains('+')
}

fn lane_for_x(x0: f32, width: f32) -> Option<DialogueLane> {
    let ratio = x0 / width;
    if (0.40..=0.51).contains(&ratio) {
        Some(DialogueLane::Main)
    } else if (0.20..=0.34).contains(&ratio) {
        Some(DialogueLane::DualLeft)
    } else if (0.58..=0.72).contains(&ratio) {
        Some(DialogueLane::DualRight)
    } else {
        None
    }
}

fn has_following_content(rows: &[Row], cue_row: usize, lane: DialogueLane, width: f32) -> bool {
    rows.iter()
        .skip(cue_row + 1)
        .take(3)
        .flat_map(|row| row.runs.iter())
        .any(|run| content_for_lane(run, lane, width))
}

fn content_for_lane(run: &VisualRun, lane: DialogueLane, width: f32) -> bool {
    let left = run.bounds.x0 / width;
    let right = run.bounds.x1 / width;
    match lane {
        DialogueLane::Main => (0.25..=0.43).contains(&left) && right <= 0.82,
        DialogueLane::DualLeft => (0.08..=0.31).contains(&left) && right < 0.50,
        DialogueLane::DualRight => (0.48..=0.75).contains(&left) && right <= 0.92,
    }
}

fn action_like(run: &VisualRun, width: f32) -> bool {
    run.bounds.x0 / width < 0.24
}

fn is_continuation_marker(text: &str) -> bool {
    let compact = text
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    matches!(compact.as_str(), "MORE" | "CONTINUED")
}

fn run_identity(run: &VisualRun) -> (usize, i32, i32) {
    (
        run.page,
        (run.baseline * 10.0).round() as i32,
        (run.bounds.x0 * 10.0).round() as i32,
    )
}

fn validate_unclaimed_dialogue(
    document: &ExtractedDocument,
    runs: &[VisualRun],
    claimed: &BTreeSet<(usize, i32, i32)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let pages_with_blocks = claimed.iter().map(|item| item.0).collect::<BTreeSet<_>>();
    for run in runs {
        if !pages_with_blocks.contains(&run.page) || claimed.contains(&run_identity(run)) {
            continue;
        }
        let Some(page) = document.pages.get(run.page - 1) else {
            continue;
        };
        let ratio = run.bounds.x0 / page.width;
        let outside_body = run.bounds.y0 > page.height * 0.88 || run.bounds.y1 < page.height * 0.08;
        if (0.285..=0.315).contains(&ratio)
            && !cue_text_shape(&run.text)
            && !is_continuation_marker(&run.text)
            && !outside_body
        {
            diagnostics.push(Diagnostic::error(
                "E_ORPHAN_DIALOGUE",
                Some(run.page),
                Some(run.text.clone()),
                Some(run.bounds),
                "dialogue-lane text is not owned by a preceding cue",
            ));
        } else if (0.33..=0.43).contains(&ratio)
            && run.text.trim_start().starts_with('(')
            && !outside_body
        {
            diagnostics.push(Diagnostic::error(
                "E_ORPHAN_DIALOGUE",
                Some(run.page),
                Some(run.text.clone()),
                Some(run.bounds),
                "parenthetical-lane text is not owned by a preceding cue",
            ));
        } else if (0.48..=0.75).contains(&ratio)
            && !outside_body
            && !looks_like_transition(&run.text)
        {
            diagnostics.push(Diagnostic::error(
                "E_UNEXPLAINED_COLUMN",
                Some(run.page),
                Some(run.text.clone()),
                Some(run.bounds),
                "body text occupies a right-hand column outside valid dual dialogue",
            ));
        }
    }
}

fn looks_like_transition(text: &str) -> bool {
    let trimmed = text.trim();
    cue_text_shape(trimmed) && trimmed.ends_with(':')
}

fn attach_printed_pages(
    document: &ExtractedDocument,
    runs: &[VisualRun],
    diagnostics: &mut [Diagnostic],
) {
    let mut printed = BTreeMap::<usize, String>::new();
    for run in runs {
        let Some(page) = document.pages.get(run.page - 1) else {
            continue;
        };
        let candidate = run.text.trim().trim_end_matches('.');
        if run.bounds.x0 > page.width * 0.75
            && run.bounds.y0 > page.height * 0.85
            && !candidate.is_empty()
            && candidate.chars().all(|ch| ch.is_ascii_digit())
        {
            printed.insert(run.page, candidate.to_string());
        }
    }
    for diagnostic in diagnostics {
        if let Some(page) = diagnostic.pdf_page {
            diagnostic.printed_page = printed.get(&page).cloned();
        }
    }
}

fn validate_block_overlaps(blocks: &[DialogueBlock], diagnostics: &mut Vec<Diagnostic>) {
    for (index, left) in blocks.iter().enumerate() {
        for right in blocks.iter().skip(index + 1) {
            if left.page != right.page
                || left.lane == right.lane
                || (left.lane != DialogueLane::Main && right.lane != DialogueLane::Main)
            {
                continue;
            }
            let left_bounds = block_bounds(left);
            let right_bounds = block_bounds(right);
            let y_overlap =
                left_bounds.y1.min(right_bounds.y1) - left_bounds.y0.max(right_bounds.y0);
            if y_overlap > 1.0 {
                diagnostics.push(Diagnostic::error(
                    "E_OVERLAPPING_BLOCKS",
                    Some(left.page),
                    Some(format!("{} / {}", left.cue_text, right.cue_text)),
                    Some(left_bounds),
                    "main and dual-dialogue blocks overlap vertically",
                ));
            }
        }
    }
}

fn block_bounds(block: &DialogueBlock) -> Bounds {
    block
        .runs
        .iter()
        .skip(1)
        .fold(block.runs[0].bounds, |bounds, run| bounds.union(run.bounds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExtractedDocument, ExtractedPage, Glyph};

    fn glyph(ch: char, x: f32, baseline: f32, index: usize) -> Glyph {
        Glyph {
            ch,
            bounds: Bounds {
                x0: x,
                y0: baseline - 2.0,
                x1: x + 7.2,
                y1: baseline + 10.0,
            },
            baseline,
            font_size: 12.0,
            angle_degrees: 0.0,
            source_index: index,
        }
    }

    fn add_text(glyphs: &mut Vec<Glyph>, text: &str, x: f32, baseline: f32) {
        let start = glyphs.len();
        for (offset, ch) in text.chars().enumerate() {
            glyphs.push(glyph(ch, x + offset as f32 * 7.2, baseline, start + offset));
        }
    }

    #[test]
    fn parser_uses_geometry_not_action_mentions() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "Mike walks away.", 72.0, 700.0);
        add_text(&mut glyphs, "MIKE (V.O.)", 270.0, 670.0);
        add_text(&mut glyphs, "(quietly)", 226.0, 658.0);
        add_text(&mut glyphs, "I heard my name.", 183.0, 646.0);
        let analysis = analyze_extracted(
            ExtractedDocument {
                pages: vec![ExtractedPage {
                    number: 1,
                    width: 612.0,
                    height: 792.0,
                    rotation_degrees: 0,
                    image_count: 0,
                    glyphs,
                    extracted_text: String::new(),
                }],
                encrypted: false,
                signed: false,
            },
            "fixture.pdf".to_string(),
            "abc".to_string(),
        );
        assert!(analysis.report.valid);
        assert_eq!(analysis.blocks.len(), 1);
        assert_eq!(analysis.blocks[0].character_key, "MIKE");
        assert_eq!(analysis.blocks[0].runs.len(), 3);
        assert!(
            analysis.blocks[0]
                .runs
                .iter()
                .all(|run| !run.text.contains("walks"))
        );
    }

    #[test]
    fn parser_treats_dual_dialogue_as_independent_blocks() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "SUSIE", 155.0, 700.0);
        add_text(&mut glyphs, "JASON", 382.0, 700.0);
        add_text(&mut glyphs, "Left.", 90.0, 688.0);
        add_text(&mut glyphs, "Right.", 316.0, 688.0);
        let analysis = analyze_extracted(
            ExtractedDocument {
                pages: vec![ExtractedPage {
                    number: 1,
                    width: 612.0,
                    height: 792.0,
                    rotation_degrees: 0,
                    image_count: 0,
                    glyphs,
                    extracted_text: String::new(),
                }],
                encrypted: false,
                signed: false,
            },
            "dual.pdf".to_string(),
            "abc".to_string(),
        );
        assert!(analysis.report.valid, "{:?}", analysis.report.diagnostics);
        assert_eq!(analysis.blocks.len(), 2);
        assert_eq!(analysis.report.characters.len(), 2);
        assert!(
            analysis
                .report
                .characters
                .iter()
                .all(|character| character.dual_blocks == 1)
        );
    }

    fn analyze_pages(pages: Vec<ExtractedPage>) -> Analysis {
        analyze_extracted(
            ExtractedDocument {
                pages,
                encrypted: false,
                signed: false,
            },
            "fixture.pdf".to_string(),
            "abc".to_string(),
        )
    }

    fn page(number: usize, glyphs: Vec<Glyph>) -> ExtractedPage {
        ExtractedPage {
            number,
            width: 612.0,
            height: 792.0,
            rotation_degrees: 0,
            image_count: 0,
            glyphs,
            extracted_text: String::new(),
        }
    }

    #[test]
    fn uppercase_dialogue_and_more_remain_in_the_block() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "RADIO HOST (V.O.)", 270.0, 700.0);
        add_text(&mut glyphs, "THIS IS STILL DIALOGUE.", 183.0, 688.0);
        add_text(&mut glyphs, "(MORE)", 270.0, 676.0);
        let analysis = analyze_pages(vec![page(1, glyphs)]);
        assert!(analysis.report.valid, "{:?}", analysis.report.diagnostics);
        assert_eq!(analysis.blocks[0].character_key, "RADIO HOST");
        assert_eq!(analysis.blocks[0].runs.len(), 3);
    }

    #[test]
    fn combined_cues_fail_closed() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "SUSIE/JASON", 270.0, 700.0);
        add_text(&mut glyphs, "Shared words.", 183.0, 688.0);
        let analysis = analyze_pages(vec![page(1, glyphs)]);
        assert!(!analysis.report.valid);
        assert!(
            analysis
                .report
                .diagnostics
                .iter()
                .any(|item| item.code == "E_AMBIGUOUS_CUE")
        );
    }

    #[test]
    fn one_sided_dual_dialogue_is_rejected() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "SUSIE", 155.0, 700.0);
        add_text(&mut glyphs, "Only left.", 90.0, 688.0);
        let analysis = analyze_pages(vec![page(1, glyphs)]);
        assert!(!analysis.report.valid);
        assert!(
            analysis
                .report
                .diagnostics
                .iter()
                .any(|item| item.code == "E_MALFORMED_DUAL_DIALOGUE")
        );
    }

    #[test]
    fn orphan_dialogue_is_rejected() {
        let mut glyphs = Vec::new();
        add_text(&mut glyphs, "MIKE", 270.0, 700.0);
        add_text(&mut glyphs, "Owned.", 183.0, 688.0);
        add_text(&mut glyphs, "Mike leaves.", 72.0, 676.0);
        add_text(&mut glyphs, "Orphaned.", 183.0, 664.0);
        let analysis = analyze_pages(vec![page(1, glyphs)]);
        assert!(!analysis.report.valid);
        assert!(
            analysis
                .report
                .diagnostics
                .iter()
                .any(|item| item.code == "E_ORPHAN_DIALOGUE")
        );
    }

    #[test]
    fn scan_rotation_and_geometry_fail_validation() {
        let mut rotated = page(1, Vec::new());
        rotated.rotation_degrees = 90;
        rotated.image_count = 1;
        let mut inconsistent = page(2, Vec::new());
        inconsistent.width = 595.0;
        let analysis = analyze_pages(vec![rotated, inconsistent]);
        let codes = analysis
            .report
            .diagnostics
            .iter()
            .map(|item| item.code.as_str())
            .collect::<BTreeSet<_>>();
        assert!(codes.contains("E_NO_TEXT_LAYER"));
        assert!(codes.contains("E_SCANNED_PAGE"));
        assert!(codes.contains("E_PAGE_ROTATION"));
        assert!(codes.contains("E_PAGE_GEOMETRY"));
    }
}
