//! Color math uses column vectors and D65 throughout. No clipping of scene highlights.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub type Matrix = [[f32; 3]; 3];
pub const IDENTITY: Matrix = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
pub const SRGB_TO_XYZ: Matrix = [
    [0.4123908, 0.35758434, 0.1804808],
    [0.212639, 0.7151687, 0.07219232],
    [0.01933082, 0.11919478, 0.95053214],
];
pub const P3_TO_XYZ: Matrix = [
    [0.48657095, 0.2656677, 0.19821729],
    [0.22897457, 0.69173855, 0.07928691],
    [0., 0.04511338, 1.0439444],
];
pub const REC2020_TO_XYZ: Matrix = [
    [0.63695806, 0.1446169, 0.16888098],
    [0.2627002, 0.67799807, 0.05930172],
    [0., 0.02807269, 1.0609851],
];
pub const ADOBE_TO_XYZ: Matrix = [
    [0.5767309, 0.185554, 0.1881852],
    [0.2973769, 0.6273491, 0.0752741],
    [0.0270343, 0.0706872, 0.9911085],
];

pub fn apply(m: Matrix, v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
pub fn inverse(m: Matrix) -> Result<Matrix> {
    let [a, b, c] = m;
    let co = [
        [
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        ],
        [
            c[1] * a[2] - c[2] * a[1],
            c[2] * a[0] - c[0] * a[2],
            c[0] * a[1] - c[1] * a[0],
        ],
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ],
    ];
    let d = a[0] * co[0][0] + a[1] * co[0][1] + a[2] * co[0][2];
    ensure!(
        d.is_finite() && d.abs() > 1e-10,
        "Singular color or geometry matrix"
    );
    Ok(std::array::from_fn(|i| {
        std::array::from_fn(|j| co[j][i] / d)
    }))
}
pub fn luminance(rgb: [f32; 3]) -> f32 {
    apply(SRGB_TO_XYZ, rgb)[1]
}

/// Normalized primary matrix from CIE xy coordinates, with unit-luminance white.
pub fn primaries_to_xyz(primaries: [[f32; 2]; 3], white: [f32; 2]) -> Result<Matrix> {
    ensure!(
        white[0] > 0. && white[1] > 0. && white[0] + white[1] < 1.,
        "Invalid color white point"
    );
    let xy_to_xyz = |xy: [f32; 2]| -> Result<[f32; 3]> {
        ensure!(
            xy.iter().all(|v| v.is_finite()) && xy[1].abs() > 1e-10,
            "Invalid color chromaticities"
        );
        Ok([xy[0] / xy[1], 1., (1. - xy[0] - xy[1]) / xy[1]])
    };
    let columns = [
        xy_to_xyz(primaries[0])?,
        xy_to_xyz(primaries[1])?,
        xy_to_xyz(primaries[2])?,
    ];
    let base = std::array::from_fn(|i| std::array::from_fn(|j| columns[j][i]));
    let scales = apply(inverse(base)?, xy_to_xyz(white)?);
    Ok(std::array::from_fn(|i| {
        std::array::from_fn(|j| base[i][j] * scales[j])
    }))
}

pub fn rgb_primaries_to_working(primaries: [[f32; 2]; 3], white: [f32; 2]) -> Result<Matrix> {
    let xyz = |xy: [f32; 2]| [xy[0] / xy[1], 1., (1. - xy[0] - xy[1]) / xy[1]];
    let m = primaries_to_xyz(primaries, white)?;
    Ok(multiply(
        inverse(SRGB_TO_XYZ)?,
        multiply(adapt_white(xyz(white), xyz([0.3127, 0.329]))?, m),
    ))
}

/// Bradford chromatic adaptation, mapping XYZ under the source white to target XYZ.
pub fn adapt_white(source: [f32; 3], target: [f32; 3]) -> Result<Matrix> {
    const B: Matrix = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let s = apply(B, source);
    let t = apply(B, target);
    ensure!(
        s.iter().all(|v| v.abs() > 1e-8),
        "Invalid adaptation white point"
    );
    let diagonal: Matrix =
        std::array::from_fn(|i| std::array::from_fn(|j| if i == j { t[i] / s[i] } else { 0. }));
    Ok(multiply(inverse(B)?, multiply(diagonal, B)))
}
pub fn srgb_encode(x: f32) -> f32 {
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
pub fn srgb_decode(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// Photographic AgX, returning display-linear sRGB coordinates with wide-gamut headroom.
pub fn agx(rgb: [f32; 3]) -> [f32; 3] {
    crate::agx::map(rgb)
}

/// Historical approximation retained for previously saved recipes.
pub fn agx_legacy(rgb: [f32; 3]) -> [f32; 3] {
    const INSET: Matrix = [
        [0.84247905, 0.0784336, 0.079223745],
        [0.042328242, 0.87846863, 0.07916613],
        [0.042375654, 0.0784336, 0.87914294],
    ];
    const OUTSET: Matrix = [
        [1.196879, -0.09802088, -0.09902974],
        [-0.05289685, 1.1519032, -0.09896118],
        [-0.052971635, -0.09804345, 1.1510737],
    ];
    let log = apply(INSET, rgb.map(|x| x.max(0.0)))
        .map(|x| ((x.max(1e-10).log2() + 12.47393) / 16.5).clamp(0.0, 1.0));
    let encoded = log.map(|x| {
        // Least-squares sixth-order fit to the AgX default contrast response.
        (((((15.5 * x - 40.14) * x + 31.96) * x - 6.868) * x + 0.4298) * x + 0.1191) * x - 0.00232
    });
    apply(OUTSET, encoded).map(|x| x.clamp(0.0, 1.0).powf(2.2))
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OutputSpace {
    #[default]
    Srgb,
    DisplayP3,
    AdobeRgb,
    Rec2020,
    LinearSrgb,
}
impl OutputSpace {
    pub fn matrix(self) -> Matrix {
        let target = match self {
            Self::Srgb | Self::LinearSrgb => SRGB_TO_XYZ,
            Self::DisplayP3 => P3_TO_XYZ,
            Self::AdobeRgb => ADOBE_TO_XYZ,
            Self::Rec2020 => REC2020_TO_XYZ,
        };
        multiply(
            inverse(target).expect("Standard matrix is invertible"),
            SRGB_TO_XYZ,
        )
    }
    pub fn encode(self, x: f32) -> f32 {
        match self {
            Self::Srgb | Self::DisplayP3 => srgb_encode(x),
            Self::AdobeRgb => x.max(0.0).powf(256.0 / 563.0),
            Self::Rec2020 => {
                if x < 0.01805397 {
                    4.5 * x
                } else {
                    1.0992968 * x.powf(0.45) - 0.0992968
                }
            }
            Self::LinearSrgb => x,
        }
    }
}

/// Monotone cubic Hermite interpolation, compiled once for every edit recipe.
pub fn curve_lut(points: &[[f32; 2]], size: usize) -> Vec<f32> {
    let d: Vec<_> = points
        .windows(2)
        .map(|p| (p[1][1] - p[0][1]) / (p[1][0] - p[0][0]))
        .collect();
    let mut slopes = vec![0.; points.len()];
    slopes[0] = d[0];
    slopes[points.len() - 1] = d[d.len() - 1];
    for i in 1..points.len() - 1 {
        if d[i - 1] * d[i] > 0.0 {
            let h0 = points[i][0] - points[i - 1][0];
            let h1 = points[i + 1][0] - points[i][0];
            let w0 = 2.0 * h1 + h0;
            let w1 = h1 + 2.0 * h0;
            slopes[i] = (w0 + w1) / (w0 / d[i - 1] + w1 / d[i]);
        }
    }
    (0..size)
        .map(|i| {
            let x = i as f32 / (size - 1) as f32;
            let j = points
                .partition_point(|p| p[0] <= x)
                .saturating_sub(1)
                .min(points.len() - 2);
            let h = points[j + 1][0] - points[j][0];
            let t = (x - points[j][0]) / h;
            let a = points[j][1];
            let b = points[j + 1][1];
            ((2. * t - 3.) * t * t + 1.) * a
                + ((t - 2.) * t + 1.) * t * h * slopes[j]
                + (-2. * t + 3.) * t * t * b
                + (t - 1.) * t * t * h * slopes[j + 1]
        })
        .collect()
}
pub fn lookup(lut: &[f32], x: f32) -> f32 {
    let t = x.clamp(0., 1.) * (lut.len() - 1) as f32;
    let i = (t as usize).min(lut.len() - 2);
    lut[i] + (lut[i + 1] - lut[i]) * (t - i as f32)
}
