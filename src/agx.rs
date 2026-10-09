//! Photographic AgX from independently sampled oracle output, with tetrahedral interpolation.
//! Working coordinates stay linear sRGB; formation is evaluated in linear Rec.2020.
use crate::color::{self, Matrix};
use std::sync::OnceLock;

pub const GRID: usize = 97;
pub const TO_REC2020: Matrix = [
    [0.6274039, 0.32928303, 0.043313067],
    [0.06909729, 0.9195404, 0.011362316],
    [0.01639144, 0.088013306, 0.89559525],
];
pub const TO_SRGB: Matrix = [
    [1.660491, -0.5876411, -0.07284986],
    [-0.12455048, 1.1328999, -0.008349423],
    [-0.018150764, -0.1005789, 1.1187297],
];
pub const LOW_LOG: f32 = 10.001408;

pub fn lattice() -> &'static [f32] {
    static DATA: OnceLock<Vec<f32>> = OnceLock::new();
    DATA.get_or_init(|| {
        let bytes = include_bytes!("data/agx-rec2020-97.f32");
        assert_eq!(bytes.len(), GRID * GRID * GRID * 3 * 4);
        bytes
            .chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
            .collect()
    })
}
pub fn shaper(x: f32) -> f32 {
    let x = x.max(0.);
    let u = if x <= 0.18 {
        0.5 * (1. + x * (1024. / 0.18)).log2() / LOW_LOG
    } else {
        0.5 + 0.5 * (x / 0.18).log2() / 16.
    };
    u.clamp(0., 1.)
}
pub fn map(rgb: [f32; 3]) -> [f32; 3] {
    map_lattice(rgb, lattice(), GRID)
}
/// Evaluate an independently prepared lattice for offline accuracy comparisons.
#[doc(hidden)]
pub fn map_lattice(rgb: [f32; 3], data: &[f32], grid: usize) -> [f32; 3] {
    let mut v = color::apply(TO_REC2020, rgb);
    let min = v.into_iter().fold(f32::INFINITY, f32::min);
    if min < 0. {
        // Move to the Rec.2020 gamut boundary along the neutral axis, retaining Y.
        let y = 0.2627002 * v[0] + 0.67799807 * v[1] + 0.05930172 * v[2];
        v = if y > 0. {
            let t = y / (y - min);
            v.map(|c| y + t * (c - y))
        } else {
            [0.; 3]
        };
    }
    let rgb = color::apply(
        TO_SRGB,
        sample(v.map(|x| shaper(x) * (grid - 1) as f32), data, grid),
    );
    // Remove tiny profile/matrix roundoff at black and white, retaining real
    // out-of-sRGB coordinates for wide-gamut output conversions.
    rgb.map(|x| {
        let edge = x.clamp(0., 1.);
        if (x - edge).abs() < 0.000002 { edge } else { x }
    })
}
fn sample(uv: [f32; 3], data: &[f32], grid: usize) -> [f32; 3] {
    let cell = uv.map(|v| (v.floor() as usize).min(grid - 2));
    let f = std::array::from_fn::<_, 3, _>(|i| uv[i] - cell[i] as f32);
    let mut order = [0, 1, 2];
    if f[order[0]] < f[order[1]] {
        order.swap(0, 1);
    }
    if f[order[1]] < f[order[2]] {
        order.swap(1, 2);
    }
    if f[order[0]] < f[order[1]] {
        order.swap(0, 1);
    }
    let weights = [
        1. - f[order[0]],
        f[order[0]] - f[order[1]],
        f[order[1]] - f[order[2]],
        f[order[2]],
    ];
    let stride = [1, grid, grid * grid];
    let mut node = cell[0] + cell[1] * grid + cell[2] * grid * grid;
    let mut out = [0.; 3];
    for (i, weight) in weights.into_iter().enumerate() {
        if i > 0 {
            node += stride[order[i - 1]];
        }
        for c in 0..3 {
            out[c] += weight * data[node * 3 + c];
        }
    }
    out
}
