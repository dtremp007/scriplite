use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::{Builder, TempDir};

use crate::analyze::analyze_pdf;
use crate::engine::PdfEngine;
use crate::error::AppError;
use crate::model::{
    Analysis, Color, GeneratedFile, GenerationManifest, GenerationSummary, normalize_character_name,
};
use crate::pdf_writer::write_highlighted_pdf;
use crate::report::write_json_atomic;

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub characters: Vec<String>,
    pub color: Color,
    pub force: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TransactionJournal {
    state: TransactionState,
    output_dir: PathBuf,
    targets: Vec<TransactionTarget>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransactionState {
    Prepared,
    Committing,
    Completed,
}

#[derive(Debug, Serialize, Deserialize)]
struct TransactionTarget {
    filename: String,
    had_original: bool,
}

pub fn generate(
    engine: &PdfEngine,
    options: &GenerateOptions,
) -> Result<GenerationManifest, AppError> {
    let analysis = analyze_pdf(engine, &options.input)?;
    if !analysis.report.valid {
        return Err(AppError::Validation);
    }
    // Journals record the output directory, so resolve it once to make recovery
    // independent of how the path was spelled or which directory we run from.
    let output_dir =
        std::path::absolute(&options.output_dir).map_err(|source| AppError::OutputIo {
            path: options.output_dir.clone(),
            source,
        })?;
    let output_dir = output_dir.as_path();
    recover_transactions(output_dir)?;
    let selected = select_characters(&analysis, &options.characters)?;
    let input_stem = options
        .input
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Usage("input filename is not valid Unicode".to_string()))?;
    let manifest_name = format!("{input_stem}.manifest.json");

    let mut plans = Vec::new();
    let mut folded_names = BTreeSet::new();
    for (key, display_name) in &selected {
        let safe_display = safe_filename_component(display_name)?;
        let filename = format!("{input_stem} ({safe_display}).pdf");
        if !folded_names.insert(filename.to_lowercase()) {
            return Err(AppError::Generation(format!(
                "multiple characters resolve to the output filename '{filename}'"
            )));
        }
        plans.push((key.clone(), display_name.clone(), filename));
    }
    if !folded_names.insert(manifest_name.to_lowercase()) {
        return Err(AppError::Generation(
            "manifest filename collides with a generated PDF".to_string(),
        ));
    }
    let targets = plans
        .iter()
        .map(|(_, _, filename)| filename.clone())
        .chain(std::iter::once(manifest_name.clone()))
        .collect::<Vec<_>>();
    if output_dir.exists() && !options.force {
        for target in &targets {
            let path = output_dir.join(target);
            if path.exists() {
                return Err(AppError::Collision(path));
            }
        }
    }

    let parent = parent_dir(output_dir);
    fs::create_dir_all(parent).map_err(|source| AppError::OutputIo {
        path: parent.to_path_buf(),
        source,
    })?;
    let staging = Builder::new()
        .prefix(".scriplite-staging-")
        .tempdir_in(parent)
        .map_err(|source| AppError::OutputIo {
            path: parent.to_path_buf(),
            source,
        })?;
    let mut generated_files = Vec::new();
    for (key, display_name, filename) in &plans {
        let blocks = analysis
            .blocks
            .iter()
            .filter(|block| &block.character_key == key)
            .collect::<Vec<_>>();
        let staged_path = staging.path().join(filename);
        write_highlighted_pdf(&options.input, &staged_path, &blocks, options.color)?;
        verify_generated(engine, &analysis, &staged_path)?;
        generated_files.push(GeneratedFile {
            character: display_name.clone(),
            filename: filename.clone(),
            sha256: hash_file(&staged_path)?,
        });
    }
    let manifest = GenerationManifest {
        schema_version: analysis.report.schema_version,
        tool_version: analysis.report.tool_version.clone(),
        source: analysis.report.source.clone(),
        valid: analysis.report.valid,
        characters: analysis.report.characters.clone(),
        diagnostics: analysis.report.diagnostics.clone(),
        generation: GenerationSummary {
            color: options.color.to_string(),
            requested_characters: selected
                .iter()
                .map(|(_, display)| display.clone())
                .collect(),
            files: generated_files,
        },
    };
    write_json_atomic(&manifest, &staging.path().join(&manifest_name))?;
    commit_batch(staging, output_dir, &targets, options.force)?;
    Ok(manifest)
}

fn select_characters(
    analysis: &Analysis,
    requested: &[String],
) -> Result<Vec<(String, String)>, AppError> {
    let available = analysis
        .report
        .characters
        .iter()
        .map(|character| {
            (
                normalize_character_name(&character.name),
                character.display_name.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if requested.is_empty() {
        return Ok(analysis
            .report
            .characters
            .iter()
            .map(|character| {
                (
                    normalize_character_name(&character.name),
                    character.display_name.clone(),
                )
            })
            .collect());
    }
    let mut seen = BTreeSet::new();
    let mut selected = Vec::new();
    for name in requested {
        let key = normalize_character_name(name);
        let Some(display) = available.get(&key) else {
            return Err(AppError::UnknownCharacter(name.clone()));
        };
        if seen.insert(key.clone()) {
            selected.push((key, display.clone()));
        }
    }
    Ok(selected)
}

fn safe_filename_component(value: &str) -> Result<String, AppError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
    {
        return Err(AppError::Generation(format!(
            "character name '{value}' cannot be used in a filename"
        )));
    }
    Ok(value.to_string())
}

fn verify_generated(
    engine: &PdfEngine,
    original: &Analysis,
    output: &Path,
) -> Result<(), AppError> {
    let generated = engine.extract(output)?;
    if generated.pages.len() != original.report.source.page_count {
        return Err(AppError::Generation(format!(
            "verification failed for '{}': page count changed",
            output.display()
        )));
    }
    for (index, page) in generated.pages.iter().enumerate() {
        if (page.width - original.report.source.width_points).abs() > 0.25
            || (page.height - original.report.source.height_points).abs() > 0.25
        {
            return Err(AppError::Generation(format!(
                "verification failed for '{}': page {} geometry changed",
                output.display(),
                index + 1
            )));
        }
        if page.extracted_text != original.page_text[index] {
            return Err(AppError::Generation(format!(
                "verification failed for '{}': page {} text changed",
                output.display(),
                index + 1
            )));
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|source| AppError::OutputIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn commit_batch(
    staging: TempDir,
    output_dir: &Path,
    targets: &[String],
    force: bool,
) -> Result<(), AppError> {
    if !output_dir.exists() {
        let staging_path = staging.keep();
        fs::rename(&staging_path, output_dir).map_err(|source| AppError::OutputIo {
            path: output_dir.to_path_buf(),
            source,
        })?;
        return Ok(());
    }
    fs::create_dir_all(output_dir).map_err(|source| AppError::OutputIo {
        path: output_dir.to_path_buf(),
        source,
    })?;
    let backup_dir = staging.path().join(".backup");
    fs::create_dir(&backup_dir).map_err(|source| AppError::OutputIo {
        path: backup_dir.clone(),
        source,
    })?;
    let journal_path = staging.path().join("transaction.json");
    let mut journal = TransactionJournal {
        state: TransactionState::Prepared,
        output_dir: output_dir.to_path_buf(),
        targets: targets
            .iter()
            .map(|filename| TransactionTarget {
                filename: filename.clone(),
                had_original: output_dir.join(filename).exists(),
            })
            .collect(),
    };
    write_journal(&journal_path, &journal)?;
    journal.state = TransactionState::Committing;
    write_journal(&journal_path, &journal)?;

    let commit_result = (|| {
        for target in &journal.targets {
            let destination = output_dir.join(&target.filename);
            if target.had_original {
                if !force {
                    return Err(AppError::Collision(destination));
                }
                fs::rename(&destination, backup_dir.join(&target.filename)).map_err(|source| {
                    AppError::OutputIo {
                        path: destination.clone(),
                        source,
                    }
                })?;
            }
            fs::rename(staging.path().join(&target.filename), &destination).map_err(|source| {
                AppError::OutputIo {
                    path: destination,
                    source,
                }
            })?;
        }
        Ok(())
    })();
    if let Err(error) = commit_result {
        rollback(staging.path(), &journal)?;
        return Err(error);
    }
    journal.state = TransactionState::Completed;
    write_journal(&journal_path, &journal)?;
    sync_directory(output_dir)?;
    Ok(())
}

fn write_journal(path: &Path, journal: &TransactionJournal) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(journal)?;
    let mut file = fs::File::create(path).map_err(|source| AppError::OutputIo {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|source| AppError::OutputIo {
            path: path.to_path_buf(),
            source,
        })
}

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn recover_transactions(output_dir: &Path) -> Result<(), AppError> {
    let parent = parent_dir(output_dir);
    let Ok(entries) = fs::read_dir(parent) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(".scriplite-staging-") {
            continue;
        }
        let journal_path = entry.path().join("transaction.json");
        let Ok(bytes) = fs::read(&journal_path) else {
            continue;
        };
        let Ok(journal) = serde_json::from_slice::<TransactionJournal>(&bytes) else {
            continue;
        };
        if journal.output_dir != output_dir {
            continue;
        }
        if journal.state != TransactionState::Completed {
            rollback(&entry.path(), &journal)?;
        }
        fs::remove_dir_all(entry.path()).map_err(|source| AppError::OutputIo {
            path: entry.path(),
            source,
        })?;
    }
    Ok(())
}

fn rollback(staging: &Path, journal: &TransactionJournal) -> Result<(), AppError> {
    for target in journal.targets.iter().rev() {
        let destination = journal.output_dir.join(&target.filename);
        let staged = staging.join(&target.filename);
        let backup = staging.join(".backup").join(&target.filename);
        if backup.exists() {
            if destination.exists() {
                fs::remove_file(&destination).map_err(|source| AppError::OutputIo {
                    path: destination.clone(),
                    source,
                })?;
            }
            fs::rename(&backup, &destination).map_err(|source| AppError::OutputIo {
                path: destination.clone(),
                source,
            })?;
        } else if !target.had_original && destination.exists() && !staged.exists() {
            fs::remove_file(&destination).map_err(|source| AppError::OutputIo {
                path: destination,
                source,
            })?;
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), AppError> {
    let directory = fs::File::open(path).map_err(|source| AppError::OutputIo {
        path: path.to_path_buf(),
        source,
    })?;
    directory.sync_all().map_err(|source| AppError::OutputIo {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_relative_paths_resolve_to_the_current_directory() {
        assert_eq!(parent_dir(Path::new("highlighted")), Path::new("."));
        assert_eq!(parent_dir(Path::new("a/highlighted")), Path::new("a"));
        assert_eq!(parent_dir(Path::new("/")), Path::new("."));
    }

    #[test]
    fn filename_component_rejects_path_syntax() {
        assert!(safe_filename_component("Mike").is_ok());
        assert!(safe_filename_component("../Mike").is_err());
        assert!(safe_filename_component("A/B").is_err());
    }

    #[test]
    fn failed_commit_restores_original_files() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("one.pdf"), b"original").unwrap();
        let staging = Builder::new()
            .prefix(".scriplite-staging-")
            .tempdir_in(root.path())
            .unwrap();
        fs::write(staging.path().join("one.pdf"), b"replacement").unwrap();

        let result = commit_batch(
            staging,
            &output,
            &["one.pdf".to_string(), "missing.pdf".to_string()],
            true,
        );
        assert!(result.is_err());
        assert_eq!(fs::read(output.join("one.pdf")).unwrap(), b"original");
        assert!(!output.join("missing.pdf").exists());
    }

    #[test]
    fn interrupted_commit_is_recovered_on_the_next_run() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("one.pdf"), b"replacement").unwrap();
        let staging = Builder::new()
            .prefix(".scriplite-staging-")
            .tempdir_in(root.path())
            .unwrap();
        let staging_path = staging.keep();
        fs::create_dir(staging_path.join(".backup")).unwrap();
        fs::write(staging_path.join(".backup/one.pdf"), b"original").unwrap();
        let journal = TransactionJournal {
            state: TransactionState::Committing,
            output_dir: output.clone(),
            targets: vec![TransactionTarget {
                filename: "one.pdf".to_string(),
                had_original: true,
            }],
        };
        write_journal(&staging_path.join("transaction.json"), &journal).unwrap();

        recover_transactions(&output).unwrap();
        assert_eq!(fs::read(output.join("one.pdf")).unwrap(), b"original");
        assert!(!staging_path.exists());
    }
}
