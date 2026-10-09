#![cfg(feature = "moebius")]
use rawpuppy::{
    models,
    moebius::{InferenceDevice, Moebius, Sampling},
    pipeline::Rendered,
};

#[test]
#[ignore = "requires prepared, pinned Moebius graphs and LibTorch runtime"]
fn native_moebius_generates_masked_pixels_and_preserves_original_samples() {
    check_generation(InferenceDevice::Auto);
}

#[test]
#[ignore = "requires prepared, pinned Moebius graphs and LibTorch runtime"]
fn prepared_graphs_run_on_cpu_after_cuda_preparation() {
    check_generation(InferenceDevice::Cpu);
}

fn check_generation(device: InferenceDevice) {
    let directory = models::cache_dir()
        .unwrap()
        .join("models/moebius/torchscript");
    let start = std::time::Instant::now();
    let model = Moebius::open(&directory, device).unwrap();
    eprintln!("model load: {:?}", start.elapsed());
    eprintln!("native device: {:?}", model.device());
    let mut pixels = vec![[0.18, 0.18, 0.18, 1.]; 512 * 512];
    let mut mask = vec![0.; 512 * 512];
    for y in 224..288 {
        for x in 224..288 {
            pixels[y * 512 + x] = [0.; 4];
            mask[y * 512 + x] = 1.;
        }
    }
    let image = Rendered {
        width: 512,
        height: 512,
        pixels,
    };
    let settings = Sampling {
        steps: 10,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let actual = model.inpaint_512(&image, &mask, &settings).unwrap();
    eprintln!("native sampling: {:?}", start.elapsed());
    let mut filled = 0;
    for (i, (a, b)) in actual.pixels.iter().zip(&image.pixels).enumerate() {
        assert!(a.iter().all(|v| v.is_finite()));
        if mask[i] == 0. {
            assert_eq!(a, b);
        } else {
            assert_eq!(a[3], 1.);
            if a[0] > 0.01 {
                filled += 1;
            }
        }
    }
    assert!(filled > 4000);
    assert_eq!(image.pixels[256 * 512 + 256], [0.; 4]);
}
