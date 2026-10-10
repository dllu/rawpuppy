use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use rawpuppy::{
    color::OutputSpace,
    edits::{Edits, Reconstruction},
    export,
    input::SensorImage,
    render::{Backend, Renderer},
    sidecar,
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
    /// Photo compute backend. Auto falls back to CPU when a device cannot handle the source.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    backend: Backend,
}
#[derive(Subcommand)]
enum Command {
    /// Open the native single-photo editor.
    Edit {
        input: Option<PathBuf>,
        #[arg(long)]
        display_profile: Option<PathBuf>,
        /// Prefer native extended-linear HDR presentation, with SDR fallback.
        #[arg(long)]
        hdr: bool,
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
        #[arg(long, value_enum)]
        reconstruction: Option<Reconstruction>,
    },
    /// Print a default JSON recipe for scripting.
    Recipe,
    /// Download and verify the Apache-2.0 local LaMa inpainting model.
    FetchLama,
    /// Download and verify the recent Moebius scene inpainting checkpoint and VAE.
    FetchMoebius,
    /// Install a verified, attributed joint reconstruction model in the local cache.
    #[cfg(feature = "raw-ml")]
    InstallRawModel { directory: PathBuf },
    /// Generate non-destructive Moebius layers for painted regions or geometric corners.
    #[cfg(feature = "moebius")]
    Inpaint {
        input: PathBuf,
        output: PathBuf,
        /// Normalized x,y,radius of an area to replace; may be repeated.
        #[arg(long,value_parser=parse_erase)]
        erase: Vec<[f32; 3]>,
        #[arg(long)]
        fill_gaps: bool,
        #[arg(long, default_value_t = 20)]
        steps: usize,
        #[arg(long, default_value_t = 0)]
        seed: i64,
        #[arg(long)]
        max_edge: Option<usize>,
        #[arg(long)]
        save_edits: bool,
        #[arg(long, value_enum, default_value = "srgb")]
        color_space: OutputSpace,
        #[arg(long)]
        overwrite: bool,
    },
    /// Experimental LaMa reference backend for comparing newer inpainting models.
    #[cfg(feature = "neural")]
    InpaintLama {
        input: PathBuf,
        output: PathBuf,
        /// Grayscale mask matching output dimensions: white replaces, black preserves.
        #[arg(long)]
        mask: Option<PathBuf>,
        #[arg(long)]
        fill_gaps: bool,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long)]
        max_edge: Option<usize>,
        #[arg(long, value_enum, default_value = "srgb")]
        color_space: OutputSpace,
        #[arg(long)]
        overwrite: bool,
    },
    /// Save a JSON recipe into an output ending in .rawpuppy.xmp.
    Save { recipe: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: this is process startup, before parsing, initializing any runtime,
        // or spawning threads. CubeCL's CUDA compiler needs a larger worker stack.
        unsafe {
            std::env::set_var("RUST_MIN_STACK", "33554432");
        }
    }
    let cli = Cli::parse();
    ensure!(cli.threads > 0, "Thread count must be nonzero");
    rayon::ThreadPoolBuilder::new()
        .num_threads(cli.threads)
        .build_global()?;
    match cli.command.unwrap_or(Command::Edit {
        input: None,
        display_profile: None,
        hdr: false,
    }) {
        Command::Edit {
            input,
            display_profile,
            hdr,
        } => rawpuppy::gui::run(input, display_profile, cli.backend, hdr)?,
        Command::Inspect { input } => {
            let start = Instant::now();
            let image = std::sync::Arc::new(SensorImage::open(&input)?);
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
            reconstruction,
        } => {
            let original = input.canonicalize()?;
            ensure!(
                output.canonicalize().ok().as_ref() != Some(&original),
                "Export cannot overwrite the original"
            );
            ensure!(max_edge != Some(0), "Maximum edge must be positive");
            let start = Instant::now();
            let image = std::sync::Arc::new(SensorImage::open(&input)?);
            let decoded = start.elapsed();
            let mut edits = if let Some(path) = explicit {
                sidecar::load(&path)?
            } else {
                sidecar::load_for_default(&input, Edits::for_image(&image))?
            };
            edits.scene.exposure += exposure;
            if let Some(method) = reconstruction {
                edits.raw.reconstruction = method;
            }
            let mut renderer = Renderer::new(cli.backend);
            renderer.set_document(input.clone());
            let rendered = renderer.render(image, &edits, max_edge)?;
            let processed = start.elapsed();
            export::write(&output, &rendered, color_space, overwrite)?;
            eprintln!("Compute: {}", renderer.label());
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
        Command::FetchLama => println!("{}", rawpuppy::models::fetch_lama()?.display()),
        Command::FetchMoebius => println!("{}", rawpuppy::models::fetch_moebius()?.display()),
        #[cfg(feature = "raw-ml")]
        Command::InstallRawModel { directory } => println!(
            "{}",
            rawpuppy::models::install_raw_model(&directory)?.display()
        ),
        #[cfg(feature = "moebius")]
        Command::Inpaint {
            input,
            output,
            erase,
            fill_gaps,
            steps,
            seed,
            max_edge,
            save_edits,
            color_space,
            overwrite,
        } => {
            ensure!(
                !erase.is_empty() || fill_gaps,
                "Supply --erase x,y,radius or --fill-gaps"
            );
            ensure!(max_edge != Some(0), "Maximum edge must be positive");
            ensure!(
                output.canonicalize().ok().as_ref() != Some(&input.canonicalize()?),
                "Inpainting cannot overwrite the original"
            );
            let image = std::sync::Arc::new(SensorImage::open(&input)?);
            let mut edits = sidecar::load_for_default(&input, Edits::for_image(&image))?;
            let mut renderer = Renderer::new(cli.backend);
            renderer.set_document(input.clone());
            let (w, h) = renderer.dimensions(image.clone(), &edits, None)?;
            let dabs: Vec<_> = erase
                .into_iter()
                .map(|p| rawpuppy::synthesis::MaskDab {
                    center: [p[0], p[1]],
                    radius: p[2],
                })
                .collect();
            let mut regions = Vec::new();
            if !dabs.is_empty() {
                regions.push((
                    rawpuppy::synthesis::brush_context(&dabs, w, h)?,
                    dabs,
                    false,
                ));
            }
            if fill_gaps {
                for region in renderer.gap_contexts(image.clone(), &edits)? {
                    regions.push((region, vec![], true));
                }
            }
            ensure!(!regions.is_empty(), "No geometric gaps to fill");
            let settings = rawpuppy::moebius::Sampling {
                steps,
                seed,
                ..Default::default()
            };
            settings.validate()?;
            let start = Instant::now();
            for (region, dabs, gaps) in regions {
                if let Some(fill) =
                    renderer.generate_fill(image.clone(), &edits, region, dabs, gaps, &settings)?
                {
                    edits.display.synthesis.push(fill);
                }
            }
            let rendered = renderer.render(image, &edits, max_edge)?;
            export::write(&output, &rendered, color_space, overwrite)?;
            if save_edits {
                sidecar::save(&sidecar::path_for(&input), &edits)?;
            }
            eprintln!(
                "Generated {} saved layers in {:.2}s → {}",
                edits.display.synthesis.len(),
                start.elapsed().as_secs_f64(),
                output.display()
            );
        }
        #[cfg(feature = "neural")]
        Command::InpaintLama {
            input,
            output,
            mask,
            fill_gaps,
            model,
            max_edge,
            color_space,
            overwrite,
        } => {
            ensure!(mask.is_some() || fill_gaps, "Supply --mask or --fill-gaps");
            ensure!(max_edge != Some(0), "Maximum edge must be positive");
            ensure!(
                output.canonicalize().ok().as_ref() != Some(&input.canonicalize()?),
                "Inpainting cannot overwrite the original"
            );
            let source = std::sync::Arc::new(SensorImage::open(&input)?);
            let edits = sidecar::load_for_default(&input, Edits::for_image(&source))?;
            let mut renderer = Renderer::new(cli.backend);
            renderer.set_document(input.clone());
            let mut rendered = renderer.render(source, &edits, max_edge)?;
            let mut values = vec![0f32; rendered.pixels.len()];
            if let Some(mask) = mask {
                ensure!(
                    output.canonicalize().ok().as_ref() != Some(&mask.canonicalize()?),
                    "Output cannot overwrite the input mask"
                );
                let mut reader = image::ImageReader::open(mask)?;
                reader.no_limits();
                let mask = reader.decode()?.to_luma32f();
                ensure!(
                    mask.width() as usize == rendered.width
                        && mask.height() as usize == rendered.height,
                    "Mask must match the developed output dimensions"
                );
                values = mask.into_raw();
                // Float luma conversion can put nominal white one ULP above one.
                ensure!(
                    values.iter().all(|v| v.is_finite()),
                    "Mask contains nonfinite samples"
                );
                values.iter_mut().for_each(|v| *v = v.clamp(0., 1.));
            }
            if fill_gaps {
                for (v, p) in values.iter_mut().zip(&rendered.pixels) {
                    *v = v.max(1. - p[3]);
                }
            }
            let model = if let Some(path) = model {
                path
            } else {
                rawpuppy::models::fetch_lama()?
            };
            let mut lama = rawpuppy::neural::Lama::open(&model)?;
            let start = Instant::now();
            let tiles = lama.inpaint(&mut rendered, &values)?;
            export::write(&output, &rendered, color_space, overwrite)?;
            eprintln!(
                "Synthesized {tiles} local tiles in {:.2}s → {}",
                start.elapsed().as_secs_f64(),
                output.display()
            );
        }
        Command::Save { recipe, output } => {
            ensure!(
                output
                    .file_name()
                    .is_some_and(|name| name.as_encoded_bytes().ends_with(b".rawpuppy.xmp")),
                "Edit sidecars must use the .rawpuppy.xmp suffix"
            );
            let edits: Edits = serde_json::from_slice(&std::fs::read(recipe)?)?;
            sidecar::save(&output, &edits)?;
        }
    }
    Ok(())
}

#[cfg(feature = "moebius")]
fn parse_erase(text: &str) -> std::result::Result<[f32; 3], String> {
    let values: Vec<f32> = text
        .split(',')
        .map(|v| v.parse::<f32>().map_err(|e| e.to_string()))
        .collect::<std::result::Result<_, _>>()?;
    if values.len() != 3 || values.iter().any(|v| !v.is_finite()) || values[2] <= 0. {
        return Err("Use x,y,radius with finite values and a positive radius".into());
    }
    Ok([values[0], values[1], values[2]])
}
