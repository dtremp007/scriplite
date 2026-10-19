use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use scriplite::batch::{GenerateOptions, generate};
use scriplite::report::{render_human_report, write_json_atomic};
use scriplite::{AppError, Color, ExitCode, PdfEngine, analyze_pdf};

#[derive(Debug, Parser)]
#[command(name = "scriplite", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect speakers, dialogue blocks, dual dialogue, and validation problems.
    Inspect {
        /// Screenplay PDF with a usable text layer.
        input: PathBuf,
        /// Also write the complete inspection report as JSON.
        #[arg(long, value_name = "REPORT.json")]
        json: Option<PathBuf>,
    },
    /// Generate one highlighted PDF per requested speaking character.
    Generate {
        /// Screenplay PDF with a usable text layer.
        input: PathBuf,
        /// Destination directory for the complete generated batch.
        #[arg(long, value_name = "DIR")]
        output_dir: PathBuf,
        /// Character to generate; repeat to select more than one.
        #[arg(long, value_name = "NAME")]
        character: Vec<String>,
        /// Highlight color as #RRGGBB or RRGGBB.
        #[arg(long, default_value = "FEFE7F")]
        color: Color,
        /// Replace batch files that already exist.
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => process::exit(code as i32),
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(error.exit_code() as i32);
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, AppError> {
    let engine = PdfEngine::bind()?;
    match cli.command {
        Command::Inspect { input, json } => {
            let analysis = analyze_pdf(&engine, &input)?;
            print!("{}", render_human_report(&analysis.report));
            if let Some(path) = json {
                write_json_atomic(&analysis.report, &path)?;
            }
            if analysis.report.valid {
                Ok(ExitCode::Success)
            } else {
                Ok(ExitCode::Validation)
            }
        }
        Command::Generate {
            input,
            output_dir,
            character,
            color,
            force,
        } => {
            let options = GenerateOptions {
                input,
                output_dir,
                characters: character,
                color,
                force,
            };
            match generate(&engine, &options) {
                Ok(manifest) => {
                    println!(
                        "Generated {} PDF{} in {}",
                        manifest.generation.files.len(),
                        if manifest.generation.files.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        options.output_dir.display()
                    );
                    for file in &manifest.generation.files {
                        println!("  {} -> {}", file.character, file.filename);
                    }
                    Ok(ExitCode::Success)
                }
                Err(AppError::Validation) => {
                    let analysis = analyze_pdf(&engine, &options.input)?;
                    print!("{}", render_human_report(&analysis.report));
                    Ok(ExitCode::Validation)
                }
                Err(error) => Err(error),
            }
        }
    }
}
