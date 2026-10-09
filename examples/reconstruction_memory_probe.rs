//! Real-model CPU allocation rejection in one address-space-limited process.
#[cfg(all(target_os = "linux", feature = "raw-ml"))]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use rawpuppy::{input::SensorImage, ml_runtime::InferenceDevice, raw_ml::BayerModel};
    use std::path::PathBuf;
    let graph = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Provide the prepared RawNIND graph directory")?,
    );
    tch::set_num_threads(2);
    tch::set_num_interop_threads(1);
    let model = BayerModel::open(&graph, InferenceDevice::Cpu)?;
    let mut source = SensorImage::from_rgb(2, 2, vec![0.18; 12])?;
    let edge = 4096usize;
    source.metadata.sensor_width = edge;
    source.metadata.sensor_height = edge;
    source.metadata.width = edge;
    source.metadata.height = edge;
    source.active = [edge; 2];
    source.cpp = 1;
    source.cfa = Some(rawler::CFA::new("RGGB"));
    source.data = vec![0.18; edge * edge];
    // Warm the model/CPU worker allocations before setting a per-process limit.
    let before = model.reconstruct_patch(&source, [256; 2], [32; 2])?;
    let vm = std::fs::read_to_string("/proc/self/status")?
        .lines()
        .find(|s| s.starts_with("VmSize:"))
        .context("Missing VmSize")?
        .split_whitespace()
        .nth(1)
        .context("Invalid VmSize")?
        .parse::<u64>()?
        * 1024;
    struct Limit(libc::rlimit);
    impl Drop for Limit {
        fn drop(&mut self) {
            // SAFETY: restore this process's previously read soft/hard limits.
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_AS, &self.0) }, 0);
        }
    }
    let mut original = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: valid writable rlimit object; affects only this process.
    ensure!(
        unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut original) } == 0,
        "Cannot read address-space limit"
    );
    let limit = libc::rlimit {
        rlim_cur: vm.checked_add(64 * 1024 * 1024).context("Limit overflow")?,
        rlim_max: original.rlim_max,
    };
    ensure!(
        limit.rlim_cur <= limit.rlim_max,
        "Existing hard limit is too low for this probe"
    );
    let limited = Limit(original);
    // SAFETY: valid rlimit; only a soft, owned-process virtual address-space limit.
    ensure!(
        unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } == 0,
        "Cannot set probe limit"
    );
    let mut progress = Vec::new();
    let failed = model.reconstruct_image(&source, false, 1024, |done, total| {
        progress.push([done, total])
    });
    drop(limited);
    let error = failed
        .err()
        .context("Expected full-cache allocation rejection")?;
    ensure!(
        progress.len() == 1 && progress[0][0] == 0,
        "A tile ran before the cache allocation rejection"
    );
    let after = model.reconstruct_patch(&source, [256; 2], [32; 2])?;
    ensure!(
        before.pixels == after.pixels,
        "Model state changed after allocation failure"
    );
    ensure!(
        source.data.iter().all(|v| *v == 0.18),
        "Source changed during failed preparation"
    );
    println!(
        "{}",
        serde_json::json!({
            "scope":"owned-process virtual address-space rejection, not system-wide memory exhaustion",
            "sensor_size":[edge,edge],"requested_camera_rgb_bytes":edge*edge*3*4,
            "vm_before_bytes":vm,"soft_limit_bytes":limit.rlim_cur,"error":format!("{error:#}"),
            "progress":progress,"source_unchanged":true,"post_failure_patch_identical":true,
        })
    );
    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "raw-ml")))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("This isolated allocation probe needs Linux and --features raw-ml")
}
