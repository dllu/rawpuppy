//! Composed Rust GPU kernel, compiled by CubeCL for CUDA, Vulkan, or Metal.
use cubecl::prelude::*;
#[cube]
fn bounded(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

#[derive(CubeType, CubeTypeMut, Clone, Copy)]
pub struct Pixel {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[cube]
fn zero() -> Pixel {
    Pixel {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 0.,
    }
}

#[cube]
fn reflect(x: i32, n: u32) -> u32 {
    if n <= 1 {
        u32::new(0)
    } else {
        let period = 2 * (n as i32 - 1);
        // Reflect is even. Reduce a positive index so backend signed-remainder
        // lowering cannot change how negative sensor-border coordinates behave.
        let t = i32::abs(x) % period;
        if t >= n as i32 {
            (period - t) as u32
        } else {
            t as u32
        }
    }
}

#[cube]
fn raw(input: &Array<f32>, d: &Array<u32>, x: i32, y: i32, c: u32) -> f32 {
    let x = reflect(x, d[0]);
    let y = reflect(y, d[1]);
    input[((y * d[0] + x) * d[2] + u32::min(c, d[2] - 1)) as usize]
}

#[cube]
fn cfa(d: &Array<u32>, x: u32, y: u32) -> u32 {
    d[12 + ((y % 2) * 2 + x % 2) as usize]
}

#[cube]
fn clean(
    input: &Array<f32>,
    d: &Array<u32>,
    p: &Array<f32>,
    x: i32,
    y: i32,
    #[comptime] detail: bool,
) -> f32 {
    if !detail {
        raw(input, d, x, y, 0)
    } else {
        let center = raw(input, d, x, y, 0);
        let mut value = center;
        if d[16] != 0 {
            let a = raw(input, d, x - 2, y, 0);
            let b = raw(input, d, x + 2, y, 0);
            let c = raw(input, d, x, y - 2, 0);
            let e = raw(input, d, x, y + 2, 0);
            let median = (a + b + c + e
                - f32::min(f32::min(a, b), f32::min(c, e))
                - f32::max(f32::max(a, b), f32::max(c, e)))
                * 0.5;
            if center > f32::max(median, 0.) * 4. + 0.02 {
                value = median;
            }
        }
        if p[37] > 0. {
            let channel = cfa(d, reflect(x, d[0]), reflect(y, d[1]));
            let mut sum = value;
            let mut weights = 1.;
            for dy in -2i32..3i32 {
                for dx in -2i32..3i32 {
                    if dx != 0 || dy != 0 {
                        let nx = reflect(x + dx, d[0]);
                        let ny = reflect(y + dy, d[1]);
                        if cfa(d, nx, ny) == channel {
                            let v = raw(input, d, x + dx, y + dy, 0);
                            let difference = (v - value) / p[37];
                            let w = (-0.5 * difference * difference
                                - 0.125 * (dx * dx + dy * dy) as f32)
                                .exp();
                            sum += w * v;
                            weights += w;
                        }
                    }
                }
            }
            value = sum / weights;
        }
        value
    }
}

#[cube]
fn reconstruct(
    input: &Array<f32>,
    d: &Array<u32>,
    p: &Array<f32>,
    x: i32,
    y: i32,
    #[comptime] mosaic: bool,
    #[comptime] detail: bool,
) -> Pixel {
    if !mosaic {
        Pixel {
            r: raw(input, d, x, y, 0),
            g: raw(input, d, x, y, 1),
            b: raw(input, d, x, y, 2),
            a: 1.,
        }
    } else {
        let center = clean(input, d, p, x, y, detail);
        let h1 = clean(input, d, p, x - 1, y, detail) + clean(input, d, p, x + 1, y, detail);
        let v1 = clean(input, d, p, x, y - 1, detail) + clean(input, d, p, x, y + 1, detail);
        let h2 = clean(input, d, p, x - 2, y, detail) + clean(input, d, p, x + 2, y, detail);
        let v2 = clean(input, d, p, x, y - 2, detail) + clean(input, d, p, x, y + 2, detail);
        let diagonals = clean(input, d, p, x - 1, y - 1, detail)
            + clean(input, d, p, x + 1, y - 1, detail)
            + clean(input, d, p, x - 1, y + 1, detail)
            + clean(input, d, p, x + 1, y + 1, detail);
        let xx = reflect(x, d[0]);
        let yy = reflect(y, d[1]);
        let channel = cfa(d, xx, yy);
        if channel == 1 {
            let horizontal = (5. * center + 4. * h1 - h2 - diagonals + 0.5 * v2) / 8.;
            let vertical = (5. * center + 4. * v1 - v2 - diagonals + 0.5 * h2) / 8.;
            if cfa(d, xx + 1, yy) == 0 {
                Pixel {
                    r: horizontal,
                    g: center,
                    b: vertical,
                    a: 1.,
                }
            } else {
                Pixel {
                    r: vertical,
                    g: center,
                    b: horizontal,
                    a: 1.,
                }
            }
        } else {
            let green = (4. * center + 2. * (h1 + v1) - h2 - v2) / 8.;
            let opposite = (6. * center + 2. * diagonals - 1.5 * (h2 + v2)) / 8.;
            if channel == 0 {
                Pixel {
                    r: center,
                    g: green,
                    b: opposite,
                    a: 1.,
                }
            } else {
                Pixel {
                    r: opposite,
                    g: green,
                    b: center,
                    a: 1.,
                }
            }
        }
    }
}

#[cube]
fn blend(a: Pixel, b: Pixel, t: f32) -> Pixel {
    Pixel {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

// CubeCL expands primitive comparisons; RangeInclusive::contains is not part of its DSL.
#[allow(clippy::manual_range_contains)]
#[cube]
fn sensor(
    input: &Array<f32>,
    d: &Array<u32>,
    p: &Array<f32>,
    u: f32,
    v: f32,
    #[comptime] mosaic: bool,
    #[comptime] detail: bool,
) -> Pixel {
    if u < 0. || u > 1. || v < 0. || v > 1. {
        zero()
    } else {
        let mut sx = u;
        let mut sy = v;
        if d[7] != 0 {
            sx = v;
            sy = u;
        }
        if d[8] != 0 {
            sx = 1. - sx;
        }
        if d[9] != 0 {
            sy = 1. - sy;
        }
        let x = d[3] as f32 + sx * d[5] as f32 - 0.5;
        let y = d[4] as f32 + sy * d[6] as f32 - 0.5;
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let tx = x - ix as f32;
        let ty = y - iy as f32;
        let a = reconstruct(input, d, p, ix, iy, mosaic, detail);
        let b = reconstruct(input, d, p, ix + 1, iy, mosaic, detail);
        let c = reconstruct(input, d, p, ix, iy + 1, mosaic, detail);
        let e = reconstruct(input, d, p, ix + 1, iy + 1, mosaic, detail);
        blend(blend(a, b, tx), blend(c, e, tx), ty)
    }
}

#[cube]
fn lens_value(p: &Array<f32>, radius2: f32, component: usize) -> f32 {
    let u =
        bounded(radius2 / crate::lens::MAX_RADIUS2, 0., 1.) * (crate::lens::LUT_SAMPLES - 1) as f32;
    let i = f32::min(u.floor(), (crate::lens::LUT_SAMPLES - 2) as f32) as usize;
    let t = u - i as f32;
    let a = p[69 + i * 2 + component];
    let b = p[69 + (i + 1) * 2 + component];
    a + t * (b - a)
}

#[cube]
fn geometry(p: &Array<f32>, u: f32, v: f32, channel: u32) -> Pixel {
    let cx = p[18] + u * p[20];
    let cy = p[19] + v * p[21];
    let a = p[0] * cx + p[1] * cy + p[2];
    let b = p[3] * cx + p[4] * cy + p[5];
    let z = p[6] * cx + p[7] * cy + p[8];
    if z <= 1e-6 {
        zero()
    } else {
        let x = a / z - 0.5;
        let y = (b / z - 0.5) * p[22];
        let r2 = 4. * (x * x + y * y) / (1. + p[22] * p[22]);
        let mut ca = 0.;
        if channel == 0 {
            ca = p[25];
        } else if channel == 2 {
            ca = p[26];
        }
        let mut camera = 1.;
        if p[67] != 0. {
            camera = lens_value(p, r2, 0);
        }
        let radial = (1. + p[23] * r2 + p[24] * r2 * r2) * (1. + ca) * camera;
        Pixel {
            r: x * radial + 0.5,
            g: y * radial / p[22] + 0.5,
            b: 0.,
            a: 1.,
        }
    }
}

#[cube]
fn agx_curve(x: f32) -> f32 {
    let x = bounded(
        (f32::max(x, 1e-10).ln() * core::f32::consts::LOG2_E + 12.47393) / 16.5,
        0.,
        1.,
    );
    (((((15.5 * x - 40.14) * x + 31.96) * x - 6.868) * x + 0.4298) * x + 0.1191) * x - 0.00232
}

#[cube]
fn tone(rgb: Pixel) -> Pixel {
    let r = f32::max(rgb.r, 0.);
    let g = f32::max(rgb.g, 0.);
    let b = f32::max(rgb.b, 0.);
    let x = agx_curve(0.84247905 * r + 0.0784336 * g + 0.079223745 * b);
    let y = agx_curve(0.042328242 * r + 0.87846863 * g + 0.07916613 * b);
    let z = agx_curve(0.042375654 * r + 0.0784336 * g + 0.87914294 * b);
    Pixel {
        r: bounded(1.196879 * x - 0.09802088 * y - 0.09902974 * z, 0., 1.).powf(2.2),
        g: bounded(-0.05289685 * x + 1.1519032 * y - 0.09896118 * z, 0., 1.).powf(2.2),
        b: bounded(-0.052971635 * x - 0.09804345 * y + 1.1510737 * z, 0., 1.).powf(2.2),
        a: 1.,
    }
}

#[cube]
fn agx_shaper(x: f32) -> f32 {
    let x = f32::max(x, 0.);
    let u = if x <= 0.18 {
        0.5 * (1. + x * (1024. / 0.18)).ln() * core::f32::consts::LOG2_E / crate::agx::LOW_LOG
    } else {
        0.5 + 0.5 * (x / 0.18).ln() * core::f32::consts::LOG2_E / 16.
    };
    bounded(u, 0., 1.) * 96.
}

#[cube]
fn agx_roundoff(x: f32) -> f32 {
    let edge = bounded(x, 0., 1.);
    if (x - edge).abs() < 0.000002 { edge } else { x }
}

#[cube]
fn photographic_tone(rgb: Pixel, p: &Array<f32>, lut: &Array<f32>) -> Pixel {
    let mut r = p[49] * rgb.r + p[50] * rgb.g + p[51] * rgb.b;
    let mut g = p[52] * rgb.r + p[53] * rgb.g + p[54] * rgb.b;
    let mut b = p[55] * rgb.r + p[56] * rgb.g + p[57] * rgb.b;
    let min = f32::min(r, f32::min(g, b));
    if min < 0. {
        let y = 0.2627002 * r + 0.67799807 * g + 0.05930172 * b;
        if y > 0. {
            let t = y / (y - min);
            r = y + t * (r - y);
            g = y + t * (g - y);
            b = y + t * (b - y);
        } else {
            r = 0.;
            g = 0.;
            b = 0.;
        }
    }
    let ur = agx_shaper(r);
    let ug = agx_shaper(g);
    let ub = agx_shaper(b);
    let ir = f32::min(ur.floor(), 95.) as usize;
    let ig = f32::min(ug.floor(), 95.) as usize;
    let ib = f32::min(ub.floor(), 95.) as usize;
    let fr = ur - ir as f32;
    let fg = ug - ig as f32;
    let fb = ub - ib as f32;
    let mut o1 = 1usize;
    let mut o2 = 98usize;
    let mut a = fr;
    let mut m = fg;
    let mut c = fb;
    if fr >= fg {
        if fg < fb {
            if fr >= fb {
                o2 = 9410;
                m = fb;
                c = fg;
            } else {
                o1 = 9409;
                o2 = 9410;
                a = fb;
                m = fr;
                c = fg;
            }
        }
    } else if fr >= fb {
        o1 = 97;
        o2 = 98;
        a = fg;
        m = fr;
        c = fb;
    } else if fg >= fb {
        o1 = 97;
        o2 = 9506;
        a = fg;
        m = fb;
        c = fr;
    } else {
        o1 = 9409;
        o2 = 9506;
        a = fb;
        m = fg;
        c = fr;
    }
    let node = ir + 97 * ig + 9409 * ib;
    let i0 = node * 3;
    let i1 = (node + o1) * 3;
    let i2 = (node + o2) * 3;
    let i3 = (node + 9507) * 3;
    let x = (1. - a) * lut[i0] + (a - m) * lut[i1] + (m - c) * lut[i2] + c * lut[i3];
    let y =
        (1. - a) * lut[i0 + 1] + (a - m) * lut[i1 + 1] + (m - c) * lut[i2 + 1] + c * lut[i3 + 1];
    let z =
        (1. - a) * lut[i0 + 2] + (a - m) * lut[i1 + 2] + (m - c) * lut[i2 + 2] + c * lut[i3 + 2];
    Pixel {
        r: agx_roundoff(p[58] * x + p[59] * y + p[60] * z),
        g: agx_roundoff(p[61] * x + p[62] * y + p[63] * z),
        b: agx_roundoff(p[64] * x + p[65] * y + p[66] * z),
        a: 1.,
    }
}

#[cube]
fn luma(rgb: Pixel) -> f32 {
    0.212639 * rgb.r + 0.7151687 * rgb.g + 0.07219232 * rgb.b
}

#[cube]
fn base(
    input: &Array<f32>,
    d: &Array<u32>,
    p: &Array<f32>,
    agx_lattice: &Array<f32>,
    u: f32,
    v: f32,
    #[comptime] mosaic: bool,
    #[comptime] detail: bool,
) -> Pixel {
    let map = geometry(p, u, v, 1);
    if map.a == 0. {
        zero()
    } else {
        let mut rgb = sensor(input, d, p, map.r, map.g, mosaic, detail);

        if p[25] != 0. {
            let point = geometry(p, u, v, 0);
            let red = sensor(input, d, p, point.r, point.g, mosaic, detail);
            rgb.a *= red.a;
            rgb.r = red.r;
        }
        if p[26] != 0. {
            let point = geometry(p, u, v, 2);
            let blue = sensor(input, d, p, point.r, point.g, mosaic, detail);
            rgb.a *= blue.a;
            rgb.b = blue.b;
        }
        if rgb.a == 0. {
            zero()
        } else {
            let x = map.r - 0.5;
            let y = (map.g - 0.5) * p[22];
            let r2 = 4. * (x * x + y * y) / (1. + p[22] * p[22]);
            let dist = (u - p[33]) * p[31] + (v - p[34]) * p[32];
            let t = bounded(0.5 + dist / p[35], 0., 1.);
            let t = t * t * (3. - 2. * t);
            let mut camera_gain = 1.;
            if p[68] != 0. {
                camera_gain = lens_value(p, r2, 1);
            }
            let gain = ((p[27] + p[28] * r2 + p[29] * r2 * r2 + p[30] * t)
                * core::f32::consts::LN_2)
                .exp()
                * camera_gain;
            let mut calibrated = Pixel {
                r: (p[9] * rgb.r + p[10] * rgb.g + p[11] * rgb.b) * gain,
                g: (p[12] * rgb.r + p[13] * rgb.g + p[14] * rgb.b) * gain,
                b: (p[15] * rgb.r + p[16] * rgb.g + p[17] * rgb.b) * gain,
                a: 1.,
            };
            if d[17] == 1 {
                calibrated = tone(calibrated);
            } else if d[17] == 2 {
                calibrated = photographic_tone(calibrated, p, agx_lattice);
            }
            let l = luma(calibrated);
            Pixel {
                r: l + (calibrated.r - l) * p[36],
                g: l + (calibrated.g - l) * p[36],
                b: l + (calibrated.b - l) * p[36],
                a: 1.,
            }
        }
    }
}

#[cube]
fn encode(x: f32) -> f32 {
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}
#[cube]
fn decode(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
#[cube]
fn curve(lut: &Array<f32>, x: f32) -> f32 {
    let t = bounded(encode(x), 0., 1.) * 4095.;
    let i = u32::min(t as u32, 4094) as usize;
    decode(lut[i] + (lut[i + 1] - lut[i]) * (t - i as f32))
}

#[cube(launch_unchecked)]
pub fn prepare(input: &Array<f32>, d: &Array<u32>, p: &Array<f32>, output: &mut Array<f32>) {
    let i = ABSOLUTE_POS;
    if i < input.len() {
        output[i] = clean(
            input,
            d,
            p,
            (i % d[0] as usize) as i32,
            (i / d[0] as usize) as i32,
            true,
        );
    }
}

#[cube(launch_unchecked)]
pub fn render(
    input: &Array<f32>,
    d: &Array<u32>,
    p: &Array<f32>,
    lut: &Array<f32>,
    agx_lattice: &Array<f32>,
    brushes: &Array<f32>,
    index: &Array<u32>,
    output: &mut Array<f32>,
    #[comptime] mosaic: bool,
    #[comptime] detail: bool,
) {
    let pixel = ABSOLUTE_POS as u32;
    if pixel >= d[19] * d[20] {
        terminate!();
    }
    let u = p[45] + p[47] * ((pixel % d[19]) as f32 + 0.5) / d[19] as f32;
    let v = p[46] + p[48] * ((pixel / d[19]) as f32 + 0.5) / d[20] as f32;
    let mut rgb = base(input, d, p, agx_lattice, u, v, mosaic, detail);
    let cx = bounded((u * 64.).floor(), 0., 63.) as usize;
    let cy = bounded((v * 64.).floor(), 0., 63.) as usize;
    let cell = cy * 64 + cx;
    let start = index[cell];
    let end = index[cell + 1];
    for j in start..end {
        let brush = index[4097 + j as usize] as usize * 10;
        let dx = (u - brushes[brush + 2]) * p[20];
        let dy = (v - brushes[brush + 3]) * p[21] * p[22];
        let distance = (dx * dx + dy * dy).sqrt() / brushes[brush + 4];
        if distance < 1. {
            let mut weight = 1.;
            if brushes[brush + 5] > 0. {
                let t = bounded((1. - distance) / brushes[brush + 5], 0., 1.);
                weight = t * t * (3. - 2. * t);
            }
            weight *= brushes[brush + 6];
            let mut replacement = base(
                input,
                d,
                p,
                agx_lattice,
                u + brushes[brush] - brushes[brush + 2],
                v + brushes[brush + 1] - brushes[brush + 3],
                mosaic,
                detail,
            );
            if replacement.a != 0. {
                replacement.r += brushes[brush + 7];
                replacement.g += brushes[brush + 8];
                replacement.b += brushes[brush + 9];
                rgb = blend(rgb, replacement, weight);
            }
        }
    }
    if d[18] != 0 {
        rgb.r = curve(lut, rgb.r);
        rgb.g = curve(lut, rgb.g);
        rgb.b = curve(lut, rgb.b);
    }
    let l = bounded(luma(rgb), 0., 1.);
    rgb.r *= 1. + p[38] * ((1. - l) * p[39] + l * p[42] - 1.);
    rgb.g *= 1. + p[38] * ((1. - l) * p[40] + l * p[43] - 1.);
    rgb.b *= 1. + p[38] * ((1. - l) * p[41] + l * p[44] - 1.);
    let i = pixel as usize * 4;
    output[i] = rgb.r;
    output[i + 1] = rgb.g;
    output[i + 2] = rgb.b;
    output[i + 3] = rgb.a;
}
