//! Real native painted-fill comparison using the saved float context and exact target.
#[cfg(feature = "moebius")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use rawpuppy::{
        color::OutputSpace,
        export,
        input::SensorImage,
        models,
        moebius::Sampling,
        render::{Backend, Renderer},
        sidecar,
        synthesis::{self, Layers},
    };
    use std::{path::PathBuf, sync::Arc, time::Instant};

    #[derive(Parser)]
    struct Args {
        input: PathBuf,
        output: PathBuf,
        /// Explicit diagnostic selection change, to cover the complete object.
        #[arg(long, default_value_t = 2.)]
        radius_scale: f32,
    }
    #[cfg(feature = "cuda")]
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        // SAFETY: startup before workers or runtimes are created.
        unsafe { std::env::set_var("RUST_MIN_STACK", "33554432") };
    }
    let args = Args::parse();
    ensure!(!args.output.exists(), "Choose a fresh result directory");
    ensure!(
        args.radius_scale.is_finite() && args.radius_scale > 0.,
        "Invalid diagnostic radius scale"
    );
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()?;
    tch::set_num_threads(8);
    tch::set_num_interop_threads(1);
    let input_hash = models::sha256(&args.input)?;
    let original_recipe_path = sidecar::path_for(&args.input);
    let recipe_hash = models::sha256(&original_recipe_path)?;
    let mut edits = sidecar::load(&original_recipe_path)?;
    let prior = edits
        .display
        .synthesis
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Provide a saved painted fill"))?;
    ensure!(
        !prior.fill_gaps,
        "Provide a painted fill for this comparison"
    );
    edits.display.synthesis.clear();
    std::fs::create_dir(&args.output)?;
    let original = args
        .output
        .join("photo")
        .with_extension(args.input.extension().unwrap_or_default());
    std::fs::copy(&args.input, &original)?;
    let source = Arc::new(SensorImage::open(&original)?);
    let mut renderer = Renderer::new(Backend::Auto);
    renderer.set_document(original.clone());
    let (w, h) = renderer.dimensions(source.clone(), &edits, None)?;
    let base = renderer.render_before_synthesis(source.clone(), &edits, prior.region, 512, 512)?;
    let mut dabs = prior.dabs.clone();
    for dab in &mut dabs {
        dab.radius *= args.radius_scale;
    }
    let settings = Sampling {
        steps: prior.steps,
        seed: prior.seed,
        guidance: prior.sampling.guidance,
        strength: prior.sampling.strength,
        noise_offset: prior.sampling.noise_offset,
    };
    let started = Instant::now();
    let fill = renderer
        .generate_fill(source, &edits, prior.region, dabs.clone(), false, &settings)?
        .unwrap();
    let generate_seconds = started.elapsed().as_secs_f64();
    ensure!(
        fill.feather == 0.15,
        "New painted fill must record inward blending"
    );
    ensure!(
        fill.harmonization == synthesis::Harmonization::BoundaryPoissonV1,
        "New painted fill must record background matching"
    );
    let mut hard_edits = edits.clone();
    let mut hard_fill = fill.clone();
    hard_fill.feather = 0.;
    hard_edits.display.synthesis.push(hard_fill);
    edits.display.synthesis.push(fill.clone());
    let mut hard = rawpuppy::pipeline::Rendered {
        width: 512,
        height: 512,
        pixels: base.pixels.clone(),
    };
    let mut smooth = rawpuppy::pipeline::Rendered {
        width: 512,
        height: 512,
        pixels: base.pixels.clone(),
    };
    let mut layers = Layers::new(original.clone());
    layers.apply(&hard_edits, &mut hard, prior.region)?;
    layers.apply(&edits, &mut smooth, prior.region)?;
    let mask = synthesis::context_mask(&base, prior.region, &dabs, false, h as f32 / w as f32);
    let aspect = h as f32 / w as f32;
    let mut blended = 0;
    let mut unselected = 0;
    let mut core = 0;
    for (i, ((a, b), before)) in hard
        .pixels
        .iter()
        .zip(&smooth.pixels)
        .zip(&base.pixels)
        .enumerate()
    {
        let uv = [
            prior.region[0] + prior.region[2] * ((i % 512) as f32 + 0.5) / 512.,
            prior.region[1] + prior.region[3] * ((i / 512) as f32 + 0.5) / 512.,
        ];
        if mask[i] == 0. {
            ensure!(a == before && b == before, "Unpainted pixels changed");
            unselected += 1;
        }
        if dabs.iter().any(|dab| {
            let dx = uv[0] - dab.center[0];
            let dy = (uv[1] - dab.center[1]) * aspect;
            dx * dx + dy * dy <= (dab.radius * (1. - fill.feather)).powi(2)
        }) {
            ensure!(a == b, "Blending changed the selected core");
            core += 1;
        }
        blended += usize::from(a != b);
        ensure!(a[3] == b[3], "Blending changed opacity");
    }
    ensure!(blended > 0, "No inward blend was exercised");
    let mut hard_boundary = 0f32;
    let mut smooth_boundary = 0f32;
    for y in 0..512 {
        for x in 0..512 {
            let i = y * 512 + x;
            for j in [
                if x < 511 { Some(i + 1) } else { None },
                if y < 511 { Some(i + 512) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if (mask[i] > 0.) != (mask[j] > 0.) {
                    for c in 0..3 {
                        hard_boundary = hard_boundary.max(
                            ((hard.pixels[i][c] - base.pixels[i][c])
                                - (hard.pixels[j][c] - base.pixels[j][c]))
                                .abs(),
                        );
                        smooth_boundary = smooth_boundary.max(
                            ((smooth.pixels[i][c] - base.pixels[i][c])
                                - (smooth.pixels[j][c] - base.pixels[j][c]))
                                .abs(),
                        );
                    }
                }
            }
        }
    }
    ensure!(
        smooth_boundary < hard_boundary,
        "Inward blend did not reduce boundary contribution"
    );
    export::write(
        &args.output.join("base.png"),
        &base,
        OutputSpace::Srgb,
        false,
    )?;
    export::write(
        &args.output.join("hard.png"),
        &hard,
        OutputSpace::Srgb,
        false,
    )?;
    export::write(
        &args.output.join("inward.png"),
        &smooth,
        OutputSpace::Srgb,
        false,
    )?;
    let recipe = sidecar::path_for(&original);
    sidecar::save(&recipe, &edits)?;
    let mut reloaded = rawpuppy::pipeline::Rendered {
        width: 512,
        height: 512,
        pixels: base.pixels.clone(),
    };
    Layers::new(original.clone()).apply(&sidecar::load(&recipe)?, &mut reloaded, prior.region)?;
    ensure!(
        reloaded.pixels == smooth.pixels,
        "Cold saved-layer reload changed blended pixels"
    );
    ensure!(
        models::sha256(&args.input)? == input_hash && models::sha256(&original)? == input_hash,
        "RAW bytes changed"
    );
    ensure!(
        models::sha256(&original_recipe_path)? == recipe_hash,
        "Original recipe changed"
    );
    let record = serde_json::json!({"scope":"one real GFX painted fill; boundary contribution, exact unselected/core preservation, opaque output and cold reload, not universal quality","input_sha256":input_hash,"original_recipe_sha256":recipe_hash,"source_dimensions":[w,h],"context":prior.region,"radius_scale":args.radius_scale,"fill":fill,"generate_seconds":generate_seconds,"unselected_pixels_exact":unselected,"selected_core_pixels_exact":core,"inward_blended_pixels":blended,"max_hard_boundary_contribution":hard_boundary,"max_inward_boundary_contribution":smooth_boundary,"cold_reload_pixels_exact":true,"alpha_exact":true,"source_and_original_recipe_unchanged":true});
    std::fs::write(
        args.output.join("receipt.json"),
        serde_json::to_string_pretty(&record)? + "\n",
    )?;
    println!("{record}");
    Ok(())
}

#[cfg(not(feature = "moebius"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Build with --features moebius and matching LibTorch 2.13")
}
