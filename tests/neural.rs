#![cfg(feature = "neural")]
use rawpuppy::{models, neural::Lama, pipeline::Rendered};

#[test]
#[ignore = "requires the pinned local LaMa model (rawpuppy fetch-lama)"]
fn real_neural_inference_fills_missing_pixels_and_preserves_every_unmasked_sample() {
    let mut lama = Lama::open(&models::lama_path().unwrap()).unwrap();
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
    let output = lama.inpaint_512(&image, &mask).unwrap();
    let mut replaced = 0;
    for (i, (a, b)) in output.pixels.iter().zip(&image.pixels).enumerate() {
        assert!(a.iter().all(|v| v.is_finite()));
        if mask[i] == 0. {
            assert_eq!(a, b);
        } else {
            assert_eq!(a[3], 1.);
            if a[0] > 0.01 {
                replaced += 1;
            }
        }
    }
    assert!(
        replaced > 4000,
        "Neural output did not reconstruct the masked constant field"
    );
    assert_eq!(image.pixels[256 * 512 + 256], [0.; 4]);
}
