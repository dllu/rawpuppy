//! Slider tracks and editable values use independent ranges.
use super::*;
use std::ops::RangeInclusive;

#[derive(Clone, Copy)]
pub(super) enum TrackScale {
    Linear,
    Logarithmic,
    FineCenter,
}

impl TrackScale {
    // An asinh track gives the center more space while staying smooth at zero.
    const KNEE: f64 = 0.85;

    fn position(self, value: f64) -> f64 {
        match self {
            Self::FineCenter => (value / Self::KNEE).asinh(),
            _ => value,
        }
    }

    fn value(self, position: f64) -> f64 {
        match self {
            Self::FineCenter => Self::KNEE * position.sinh(),
            _ => position,
        }
    }
}

pub(super) fn exposure_slider(ui: &mut egui::Ui, value: &mut f32) -> egui::Response {
    parameter_slider(
        ui,
        value,
        -4. ..=4.,
        -32. ..=32.,
        "Exposure",
        " EV",
        TrackScale::FineCenter,
    )
}

pub(super) fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) -> egui::Response {
    let numeric = if suffix == " EV" {
        -32. ..=32.
    } else {
        range.clone()
    };
    linear_slider(ui, value, range, numeric, label, suffix)
}

pub(super) fn angle_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
) -> egui::Response {
    linear_slider(ui, value, range, -f32::MAX..=f32::MAX, label, "°")
}

pub(super) fn linear_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    track: RangeInclusive<f32>,
    numeric: RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) -> egui::Response {
    parameter_slider(ui, value, track, numeric, label, suffix, TrackScale::Linear)
}

pub(super) fn parameter_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    track: RangeInclusive<f32>,
    numeric: RangeInclusive<f32>,
    label: &str,
    suffix: &str,
    scale: TrackScale,
) -> egui::Response {
    ui.push_id(label, |ui| {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            let span = f64::from(*track.end()) - f64::from(*track.start());
            let step = match suffix {
                "°" => Some(0.1),
                " EV" => Some(0.01),
                _ => None,
            };
            let speed = if suffix == " K" {
                10.
            } else {
                step.unwrap_or(span / 500.)
            };
            let numeric = ui
                .horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let input = egui::DragValue::new(value)
                            .speed(speed)
                            .range(numeric)
                            .clamp_existing_to_range(false)
                            .suffix(suffix)
                            .max_decimals(if suffix == " K" {
                                0
                            } else if span < 0.2 {
                                4
                            } else {
                                3
                            });
                        ui.add(if step.is_some() {
                            input.min_decimals(2)
                        } else {
                            input
                        })
                        .on_hover_text("Click the value to type, or drag it for fine adjustment")
                    })
                    .inner
                })
                .inner;

            ui.spacing_mut().slider_width = ui.available_width();
            let min = f64::from(*track.start());
            let max = f64::from(*track.end());
            let control =
                egui::Slider::from_get_set(scale.position(min)..=scale.position(max), |position| {
                    if let Some(position) = position {
                        let next = scale.value(position);
                        let next = step.map_or(next, |step| (next / step).round() * step);
                        *value = next.clamp(min, max) as f32;
                    }
                    scale.position(f64::from(*value))
                })
                .show_value(false)
                .clamping(egui::SliderClamping::Never)
                .logarithmic(matches!(scale, TrackScale::Logarithmic))
                .trailing_fill(!matches!(scale, TrackScale::FineCenter));
            let control = if matches!(scale, TrackScale::Linear)
                && let Some(step) = step
            {
                control.step_by(step)
            } else {
                control
            };
            let track = ui.add(control);
            if matches!(scale, TrackScale::FineCenter) {
                let center = Pos2::new(track.rect.center().x, track.rect.bottom() - 5.);
                ui.painter().line_segment(
                    [center, center + Vec2::new(0., 4.)],
                    egui::Stroke::new(1., ui.visuals().weak_text_color()),
                );
            }
            track.union(numeric)
        })
        .inner
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_center_track_is_symmetric_and_moves_more_slowly_around_zero() {
        let scale = TrackScale::FineCenter;
        let half = scale.position(4.) * 0.5;
        assert_eq!(scale.value(0.), 0.);
        assert!(scale.value(half).abs() < 1.25);
        assert!((scale.value(-half) + scale.value(half)).abs() < 1e-12);
        for ev in [-32., -4., -0.5, 0.5, 4., 32.] {
            assert!((scale.value(scale.position(ev)) - ev).abs() < 1e-12);
        }
    }
}
