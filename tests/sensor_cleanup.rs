use rawpuppy::{edits::RawEdits, input::SensorImage};

fn sensor(pattern: &str, width: usize, height: usize, rgb: [f32; 3]) -> SensorImage {
    let mut image = SensorImage::from_rgb(width, height, vec![0.; width * height * 3]).unwrap();
    let cfa = rawler::CFA::new(pattern);
    image.data = (0..width * height)
        .map(|i| rgb[cfa.color_at(i / width, i % width)])
        .collect();
    image.cpp = 1;
    image.cfa = Some(cfa);
    image
}
const XTRANS: &str = "GRGGBGBGBRGRGRGGBGGBGGRGRGRBGBGBGGRG";

#[test]
fn denoising_a_repaired_sensor_matches_denoising_a_sensor_without_the_defect() {
    for pattern in ["RGGB", "BGGR", "GRBG", "GBRG", XTRANS] {
        let reference = sensor(pattern, 25, 25, [0.02, 0.04, 0.08]);
        for (x, y) in [(12, 12), (13, 12), (12, 13), (13, 13), (14, 13), (14, 14)] {
            let mut defective = sensor(pattern, 25, 25, [0.02, 0.04, 0.08]);
            defective.data[y * 25 + x] = 1.;
            let before = defective.data.clone();
            let repaired = RawEdits {
                hot_pixels: true,
                denoise: 0.5,
                ..Default::default()
            };
            let original = RawEdits {
                denoise: 0.5,
                ..Default::default()
            };
            for yy in 8..18 {
                for xx in 8..18 {
                    let actual = defective.reconstruct(xx, yy, &repaired);
                    let expected = reference.reconstruct(xx, yy, &original);
                    for c in 0..3 {
                        assert!(
                            (actual[c] - expected[c]).abs() < 2e-6,
                            "{pattern} defect {x},{y}, sample {xx},{yy}: {actual:?} != {expected:?}"
                        );
                    }
                }
            }
            assert_eq!(defective.data, before);
        }
    }
}

#[test]
fn xtrans_hot_pixel_detection_preserves_color_planes_at_every_phase_and_border() {
    let rgb = [0.8, 0.015, 0.09];
    let image = sensor(XTRANS, 18, 18, rgb);
    let before = image.data.clone();
    let edits = RawEdits {
        hot_pixels: true,
        denoise: 0.5,
        ..Default::default()
    };
    for y in 0..18 {
        for x in 0..18 {
            let pixel = image.reconstruct(x, y, &edits);
            for c in 0..3 {
                assert!((pixel[c] - rgb[c]).abs() < 2e-6, "Phase {x},{y}: {pixel:?}");
            }
        }
    }
    assert_eq!(image.data, before);
}
