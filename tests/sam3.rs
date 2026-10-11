#![cfg(feature = "sam3")]
use rawpuppy::{color, ml_runtime::InferenceDevice, pipeline::Rendered, sam3::Model};

#[test]
#[ignore = "requires the attributed local SAM 3 graph and an owned architectural preview"]
fn real_native_masks_agree_on_cpu_and_the_selected_device() {
    tch::set_num_threads(8);
    let directory =
        std::env::var_os("RAWPUPPY_TEST_SAM3_GRAPH").expect("Set RAWPUPPY_TEST_SAM3_GRAPH");
    let path = std::env::var_os("RAWPUPPY_TEST_SAM3_IMAGE").expect("Set RAWPUPPY_TEST_SAM3_IMAGE");
    let image = image::open(path).unwrap().to_rgb8();
    let preview = Rendered {
        width: image.width() as usize,
        height: image.height() as usize,
        pixels: image
            .pixels()
            .map(|p| {
                [
                    color::srgb_decode(p[0] as f32 / 255.),
                    color::srgb_decode(p[1] as f32 / 255.),
                    color::srgb_decode(p[2] as f32 / 255.),
                    1.,
                ]
            })
            .collect(),
    };
    let preference = if std::env::var_os("RAWPUPPY_TEST_SAM3_CUDA").is_some() {
        InferenceDevice::Cuda
    } else {
        InferenceDevice::Auto
    };
    let selected = Model::open(std::path::Path::new(&directory), preference).unwrap();
    let cpu = Model::open(std::path::Path::new(&directory), InferenceDevice::Cpu).unwrap();
    let expected = cpu.mask(&preview).unwrap();
    drop(cpu);
    let actual = selected.mask(&preview).unwrap();
    let differences = actual
        .values
        .iter()
        .zip(&expected.values)
        .filter(|(a, b)| a != b)
        .count();
    let support = actual.values.iter().filter(|&&v| v > 0.5).count();
    println!(
        "SAM 3 {:?}: {support} architectural pixels, {differences} CPU disagreements / {} pixels",
        selected.device(),
        actual.values.len()
    );
    assert!(
        support > actual.values.len() / 20,
        "No meaningful architectural mask"
    );
    assert!(
        differences * 1000 <= actual.values.len(),
        "Native mask differs from CPU by more than 0.1%"
    );
}
