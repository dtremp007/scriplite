mod common;

use scriplite::batch::{GenerateOptions, generate};
use scriplite::{AppError, Color, PdfEngine, analyze_pdf};

use common::{count_highlight_rectangles, fixture, lock};

const SOURCE: &str = "Dee Clock - 2026-07-25.pdf";
const REFERENCE: &str = "Dee Clock (Mike).pdf";
const DEFAULT_FILL: &str = "0.996078 0.996078 0.498039 rg";

/// The screenplay fixtures here are private and are not distributed with the
/// source, so these tests skip without them; tests/sample_screenplay.rs covers
/// the same paths with a public fixture.
fn engine_with_fixtures(names: &[&str]) -> Option<PdfEngine> {
    let engine = common::bind_engine()?;
    if let Some(missing) = names.iter().find(|name| !fixture(name).is_file()) {
        eprintln!("skipping real-world PDF test because fixture '{missing}' is missing");
        return None;
    }
    Some(engine)
}

#[test]
fn dee_clock_detection_matches_reference_counts() {
    let _guard = lock();
    let Some(engine) = engine_with_fixtures(&[SOURCE]) else {
        return;
    };
    let analysis = analyze_pdf(&engine, &fixture(SOURCE)).unwrap();
    assert!(analysis.report.valid, "{:?}", analysis.report.diagnostics);
    assert_eq!(analysis.report.characters.len(), 12);

    let mike = analysis
        .report
        .characters
        .iter()
        .find(|character| character.name == "MIKE")
        .unwrap();
    assert_eq!(mike.cue_blocks, 70);
    assert_eq!(mike.highlighted_visual_lines, 213);
    assert!(
        analysis
            .blocks
            .iter()
            .filter(|block| block.character_key == "MIKE")
            .all(|block| block.runs.iter().all(|run| run.bounds.x0 > 150.0))
    );

    for name in ["SUSIE", "JASON"] {
        let character = analysis
            .report
            .characters
            .iter()
            .find(|character| character.name == name)
            .unwrap();
        assert_eq!(character.dual_blocks, 1);
    }
}

#[test]
fn generated_mike_pdf_preserves_text_and_has_213_yellow_rectangles() {
    let _guard = lock();
    let Some(engine) = engine_with_fixtures(&[SOURCE]) else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let output_dir = temporary.path().join("batch");
    let options = GenerateOptions {
        input: fixture(SOURCE),
        output_dir: output_dir.clone(),
        characters: vec!["Mike".to_string()],
        color: Color::default(),
        force: false,
    };
    let manifest = generate(&engine, &options).unwrap();
    assert_eq!(manifest.generation.files.len(), 1);
    let output = output_dir.join(&manifest.generation.files[0].filename);
    assert!(output.is_file());
    assert!(
        output_dir
            .join("Dee Clock - 2026-07-25.manifest.json")
            .is_file()
    );
    assert_eq!(count_highlight_rectangles(&output, DEFAULT_FILL), 213);

    let collision = generate(&engine, &options).unwrap_err();
    assert!(matches!(collision, AppError::Collision(_)));

    let mut forced = options;
    forced.force = true;
    generate(&engine, &forced).unwrap();
    assert_eq!(count_highlight_rectangles(&output, DEFAULT_FILL), 213);
}

#[test]
fn reference_files_have_identical_extracted_words() {
    let _guard = lock();
    let Some(engine) = engine_with_fixtures(&[SOURCE, REFERENCE]) else {
        return;
    };
    let source = engine.extract(&fixture(SOURCE)).unwrap();
    let reference = engine.extract(&fixture(REFERENCE)).unwrap();
    let words = |document: &scriplite::model::ExtractedDocument| {
        document
            .pages
            .iter()
            .flat_map(|page| page.extracted_text.split_whitespace().map(str::to_string))
            .collect::<Vec<_>>()
    };
    assert_eq!(words(&source), words(&reference));
}
