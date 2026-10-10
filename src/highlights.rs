//! Sensor-informed neutral estimate for missing camera-channel intensity.
pub const MASK_THRESHOLD: f32 = 0.98;
pub const FULL_THRESHOLD: f32 = 0.995;

/// Intact channels remain exact. A clipped channel is a lower bound, so its
/// estimate can only increase it. With no intact channel, use a neutral estimate
/// at the largest white-balanced lower bound. This does not recover lost detail.
pub fn recover(rgb: [f32; 3], mask: [f32; 3], white: [f32; 3]) -> [f32; 3] {
    if mask == [0.; 3] {
        return rgb;
    }
    let balanced: [f32; 3] = std::array::from_fn(|c| rgb[c] * white[c]);
    let mut sum = 0.;
    let mut count = 0.;
    for (c, value) in balanced.iter().enumerate() {
        let valid = 1. - mask[c];
        sum += value * valid;
        count += valid;
    }
    let lower = balanced.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let target = if count > 0. {
        lower + count.min(1.) * (sum / count - lower)
    } else {
        lower
    };
    std::array::from_fn(|c| {
        if mask[c] == 0. {
            rgb[c]
        } else {
            let t = ((rgb[c] - MASK_THRESHOLD) / (FULL_THRESHOLD - MASK_THRESHOLD)).clamp(0., 1.);
            rgb[c] + mask[c] * t * t * (3. - 2. * t) * (target / white[c] - rgb[c]).max(0.)
        }
    })
}
