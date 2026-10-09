//! Exercise the actual editor's SDR and optional HDR paths in owned native windows.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use rawpuppy::{
    color::OutputSpace,
    edits::{Edits, ToneMapper},
    export,
    pipeline::Rendered,
    sidecar,
};
use std::{path::PathBuf, process::Command};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    editor: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    require_hdr: bool,
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        !args.output.exists(),
        "Choose a fresh editor validation directory"
    );
    std::fs::create_dir_all(&args.output)?;
    let output = args.output.canonicalize()?;
    let editor = args.editor.canonicalize()?;
    let image = Rendered {
        width: 32,
        height: 16,
        pixels: (0..32 * 16)
            .map(|i| {
                [
                    [-0.125, 0.5, 2., 1.],
                    [4., 2., 1., 1.],
                    [0.18, 0.18, 0.18, 1.],
                    [1.; 4],
                ][i % 32 / 8]
            })
            .collect(),
    };
    let source = output.join("source.exr");
    export::write(&source, &image, OutputSpace::LinearSrgb, false)?;
    let mut edits = Edits::default();
    edits.tone.mapper = ToneMapper::Linear;
    let sidecar_path = sidecar::path_for(&source);
    sidecar::save(&sidecar_path, &edits)?;
    let original = rawpuppy::models::sha256(&source)?;
    let recipe = rawpuppy::models::sha256(&sidecar_path)?;
    for hdr in [false, true] {
        let report_path = output.join(if hdr { "hdr.json" } else { "sdr.json" });
        let log_path = output.join(if hdr { "hdr.log" } else { "sdr.log" });
        let log = std::fs::File::create(&log_path)?;
        let mut command = Command::new(&editor);
        command
            .args(["--backend", "cpu", "edit"])
            .arg(&source)
            .env("RAWPUPPY_TEST_GUI_REPORT", &report_path)
            .stdout(log.try_clone()?)
            .stderr(log);
        if hdr {
            command.arg("--hdr");
        }
        let status = command.status()?;
        ensure!(
            status.success(),
            "Native editor exited {status}:\n{}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).with_context(|| {
                format!(
                    "No editor frame report:\n{}",
                    std::fs::read_to_string(&log_path).unwrap_or_default()
                )
            })?)?;
        let active = report["hdr_active"]
            .as_bool()
            .context("No presentation mode in report")?;
        if !hdr {
            ensure!(!active, "SDR editor unexpectedly selected HDR");
        }
        if active {
            let scale = report["white_scale"]
                .as_f64()
                .context("No HDR white scale")?;
            let pixels = report["float_signal"]["photo_samples"]
                .as_array()
                .context("No float photo samples")?;
            ensure!(pixels.len() == 4, "Missing native photo samples");
            for (actual, expected) in
                pixels
                    .iter()
                    .zip([[-0.125, 0.5, 2.], [4., 2., 1.], [0.18; 3], [1.; 3]])
            {
                for c in 0..3 {
                    let signal = actual[c].as_f64().context("Invalid native sample")?;
                    ensure!(
                        (signal / scale - expected[c]).abs() < 0.006,
                        "Native HDR source sample changed: {actual}"
                    );
                }
            }
            ensure!(
                report["float_signal"]["maximum_rgb"].as_f64().unwrap() > scale * 3.9,
                "HDR editor clipped highlights"
            );
        } else if hdr && args.require_hdr {
            anyhow::bail!("Native editor did not select an HDR surface");
        }
        ensure!(
            rawpuppy::models::sha256(&source)? == original,
            "Editor modified the input"
        );
        ensure!(
            rawpuppy::models::sha256(&sidecar_path)? == recipe,
            "Display preference modified the recipe"
        );
        println!(
            "{}",
            serde_json::json!({"hdr_requested":hdr,"hdr_active":active,"normal_exit":true,"source_and_recipe_unchanged":true})
        );
    }
    Ok(())
}
