use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use rawpuppy::{
    color::OutputSpace, edits::Edits, export, input::SensorImage, pipeline::Pipeline, sidecar,
};
use std::{path::PathBuf, time::Instant};

#[derive(Parser)]
#[command(version, about = "A minimal, non-destructive RAW photo editor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Limit workers to share the workstation.
    #[arg(long, global = true, default_value_t = 8)]
    threads: usize,
}
#[derive(Subcommand)]
enum Command {
    /// Open the native single-photo editor.
    Edit {
        input: Option<PathBuf>,
        #[arg(long)]
        display_profile: Option<PathBuf>,
    },
    /// Decode and inspect camera calibration and dimensions.
    Inspect { input: PathBuf },
    /// Apply the fixed pipeline and export without changing the original.
    Export {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        sidecar: Option<PathBuf>,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0.0)]
        exposure: f32,
        #[arg(long)]
        max_edge: Option<usize>,
        #[arg(long, value_enum, default_value = "srgb")]
        color_space: OutputSpace,
        #[arg(long)]
        overwrite: bool,
    },
    /// Print a default JSON recipe for scripting.
    Recipe,
    /// Save a JSON recipe into a Rawpuppy XMP sidecar.
    Save { recipe: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(cli.threads > 0, "Thread count must be nonzero");
    rayon::ThreadPoolBuilder::new()
        .num_threads(cli.threads)
        .build_global()?;
    match cli.command.unwrap_or(Command::Edit {
        input: None,
        display_profile: None,
    }) {
        Command::Edit {
            input,
            display_profile,
        } => rawpuppy::gui::run(input, display_profile)?,
        Command::Inspect { input } => {
            let start = Instant::now();
            let image = SensorImage::open(&input)?;
            println!("{}", serde_json::to_string_pretty(&image.metadata)?);
            eprintln!(
                "Decoded in {:.2}s; {:.1} MiB sensor allocation",
                start.elapsed().as_secs_f64(),
                image.data.len() as f64 * 4. / 1048576.
            );
        }
        Command::Export {
            input,
            output,
            sidecar: explicit,
            exposure,
            max_edge,
            color_space,
            overwrite,
        } => {
            let original = input.canonicalize()?;
            ensure!(
                output.canonicalize().ok().as_ref() != Some(&original),
                "Export cannot overwrite the original"
            );
            ensure!(max_edge != Some(0), "Maximum edge must be positive");
            let start = Instant::now();
            let image = SensorImage::open(&input)?;
            let decoded = start.elapsed();
            let mut edits = if let Some(path) = explicit {
                sidecar::load(&path)?
            } else {
                sidecar::load_for(&input)?
            };
            edits.scene.exposure += exposure;
            let rendered = Pipeline::compile(&image, &edits)?.render(max_edge)?;
            let processed = start.elapsed();
            export::write(&output, &rendered, color_space, overwrite)?;
            eprintln!(
                "{} × {} → {} · decode {:.2}s · render {:.2}s · export {:.2}s",
                rendered.width,
                rendered.height,
                output.display(),
                decoded.as_secs_f64(),
                (processed - decoded).as_secs_f64(),
                (start.elapsed() - processed).as_secs_f64()
            );
        }
        Command::Recipe => println!("{}", serde_json::to_string_pretty(&Edits::default())?),
        Command::Save { recipe, output } => {
            let edits: Edits = serde_json::from_slice(&std::fs::read(recipe)?)?;
            sidecar::save(&output, &edits)?;
        }
    }
    Ok(())
}
