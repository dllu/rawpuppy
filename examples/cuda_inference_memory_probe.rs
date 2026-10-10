//! Real-model allocation rejection within this process's caching-allocator budget.
#[cfg(all(target_os = "linux", feature = "raw-ml", feature = "moebius"))]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use rawpuppy::{
        input::SensorImage,
        ml_runtime::InferenceDevice,
        moebius::{Moebius, Sampling},
        raw_ml::BayerModel,
    };
    use std::{ffi::CStr, path::PathBuf};
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(
        args.len() == 3,
        "Provide diagnostic bridge, RawNIND graph and Moebius graph directories"
    );
    tch::set_num_threads(2);
    tch::set_num_interop_threads(1);
    // SAFETY: the explicitly supplied diagnostic bridge uses the matching
    // LibTorch ABI and remains loaded until every restored limit is dropped.
    let library = unsafe { libloading::Library::new(&args[0]) }?;
    type Budget = unsafe extern "C" fn(u64, *mut f64, *mut std::ffi::c_char, u64) -> i32;
    type Restore = unsafe extern "C" fn(f64) -> i32;
    let budget: libloading::Symbol<Budget> = unsafe { library.get(b"rawpuppy_cuda_budget\0") }?;
    let restore: libloading::Symbol<Restore> = unsafe { library.get(b"rawpuppy_cuda_restore\0") }?;
    struct Limit {
        previous: f64,
        restore: Restore,
    }
    impl Drop for Limit {
        fn drop(&mut self) {
            assert_eq!(
                unsafe { (self.restore)(self.previous) },
                0,
                "Failed to restore owned-process CUDA allocator"
            );
        }
    }
    let limit = || -> anyhow::Result<Limit> {
        let mut previous = 0.;
        let mut error = [0 as std::ffi::c_char; 2048];
        let result = unsafe {
            budget(
                64 * 1024 * 1024,
                &mut previous,
                error.as_mut_ptr(),
                error.len() as u64,
            )
        };
        ensure!(
            result == 0,
            "{}",
            unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
        );
        Ok(Limit {
            previous,
            restore: *restore,
        })
    };
    let raw = BayerModel::open(&args[1], InferenceDevice::Cuda)?;
    let mut source = SensorImage::from_rgb(1024, 1024, vec![0.18; 1024 * 1024 * 3])?;
    source.cpp = 1;
    source.cfa = Some(rawler::CFA::new("RGGB"));
    source.data = vec![0.18; 1024 * 1024];
    let before = raw.reconstruct_patch(&source, [256; 2], [32; 2])?;
    let limited = limit()?;
    let failure = raw.reconstruct_patch(&source, [256; 2], [512; 2]);
    drop(limited);
    let raw_error = failure
        .err()
        .context("Expected capped CUDA reconstruction failure")?;
    ensure!(
        format!("{raw_error:#}")
            .to_lowercase()
            .contains("out of memory"),
        "Failure was not CUDA allocation rejection: {raw_error:#}"
    );
    let after = raw.reconstruct_patch(&source, [256; 2], [32; 2])?;
    ensure!(
        before.pixels == after.pixels,
        "RawNIND changed after allocator rejection"
    );
    ensure!(
        source.data.iter().all(|v| *v == 0.18),
        "Original sensor changed"
    );
    drop(raw);
    let model = Moebius::open(&args[2], InferenceDevice::Cuda)?;
    let image = rawpuppy::pipeline::Rendered {
        width: 512,
        height: 512,
        pixels: vec![[0.18, 0.18, 0.18, 1.]; 512 * 512],
    };
    let mut mask = vec![0.; 512 * 512];
    for y in 248..264 {
        for x in 248..264 {
            mask[y * 512 + x] = 1.;
        }
    }
    let settings = Sampling {
        steps: 2,
        ..Default::default()
    };
    let before = model.inpaint_512(&image, &mask, &settings)?;
    let limited = limit()?;
    let failure = model.inpaint_512(&image, &mask, &settings);
    drop(limited);
    let moebius_error = failure
        .err()
        .context("Expected capped CUDA synthesis failure")?;
    ensure!(
        format!("{moebius_error:#}")
            .to_lowercase()
            .contains("out of memory"),
        "Failure was not CUDA allocation rejection: {moebius_error:#}"
    );
    let after = model.inpaint_512(&image, &mask, &settings)?;
    ensure!(
        before.pixels == after.pixels,
        "Seeded Moebius changed after allocator rejection"
    );
    println!(
        "{}",
        serde_json::json!({"scope":"64 MiB caching-allocator cap in one owned process; not device-wide exhaustion","budget_bytes":64*1024*1024,"raw_error":format!("{raw_error:#}"),"moebius_error":format!("{moebius_error:#}"),"raw_recovered_identically":true,"seeded_synthesis_recovered_identically":true,"source_unchanged":true})
    );
    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "raw-ml", feature = "moebius")))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("This owned-process CUDA probe needs Linux and --features raw-ml,moebius");
}
