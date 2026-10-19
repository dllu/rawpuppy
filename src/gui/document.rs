//! Document actions, history integration and replies scoped to the current photo.
use super::*;

impl Editor {
    pub(super) fn send(&mut self, work: Work) {
        let saving = matches!(work, Work::Save { .. });
        let tx = if saving { &self.save_tx } else { &self.tx };
        if tx.send(work).is_err() {
            self.error = Some(
                if saving {
                    "Sidecar writer stopped"
                } else {
                    "Photo worker stopped"
                }
                .into(),
            );
            self.busy = None;
            if saving {
                self.autosave.in_flight = None;
                self.autosave.failed = true;
                self.close_after_save = false;
            }
        }
    }
    pub(super) fn open(&mut self, path: PathBuf) {
        self.load_generation = self.load_generation.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        self.busy = Some(Busy::Load(self.load_generation));
        self.status = "Decoding photograph…".into();
        self.send(Work::Open {
            id: self.load_generation,
            path,
        });
    }
    pub(super) fn request_open(&mut self, path: PathBuf) {
        self.finish_edit_gesture();
        if self.dirty() {
            self.pending = Some(Pending::Open(path));
            self.close_after_save = true;
            self.save();
        } else {
            self.open(path);
        }
    }
    pub(super) fn choose_photo(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open photograph")
            .add_filter(
                "Photographs",
                &[
                    "raf", "dng", "nef", "cr2", "cr3", "arw", "orf", "rw2", "pef", "srw", "raw",
                    "jpg", "jpeg", "png", "tif", "tiff", "exr",
                ],
            )
            .pick_file()
        {
            self.request_open(path);
        }
    }
    pub(super) fn remember(&mut self, previous: Edits) {
        if self.history.record(previous, &self.edits) {
            self.changed();
        }
    }
    pub(super) fn finish_edit_gesture(&mut self) {
        if self.history.finish_gesture(&self.edits) {
            self.changed();
        }
    }
    pub(super) fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.preview_pending = true;
    }
    pub(super) fn undo(&mut self) {
        self.framing.cancel_drag();
        self.adjustment_drag = None;
        if self.history.undo(&mut self.edits) {
            self.sync_framing_aspect();
            self.changed();
        }
    }
    pub(super) fn auto_perspective(&mut self, mode: crate::perspective::Mode) {
        self.start_perspective(mode, false, true);
    }
    pub(super) fn start_perspective(
        &mut self,
        mode: crate::perspective::Mode,
        manual: bool,
        apply: bool,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.finish_edit_gesture();
        if let Some(image) = &self.image {
            let image = image.clone();
            self.busy = Some(Busy::Perspective(self.load_generation));
            self.status = "Finding straight architectural edges…".into();
            self.perspective_status = None;
            self.send(Work::Perspective {
                load_id: self.load_generation,
                revision: self.history.revision(),
                image,
                edits: self.edits.clone(),
                mode,
                provider: if self.perspective_mask {
                    crate::perspective::MaskProvider::Auto
                } else {
                    crate::perspective::MaskProvider::Classical
                },
                manual,
                apply,
                crop: self
                    .framing
                    .crop_constraint(self.edits.geometry.crop_aspect),
            });
        }
    }
    pub(super) fn auto_crop(&mut self, constraint: crate::crop::Constraint) {
        if self.busy.is_some() {
            return;
        }
        self.finish_edit_gesture();
        if let Some(image) = self.image.clone() {
            self.busy = Some(Busy::AutoCrop(self.load_generation));
            self.status = "Finding the largest crop without empty corners…".into();
            self.send(Work::AutoCrop {
                load_id: self.load_generation,
                revision: self.history.revision(),
                image,
                edits: self.edits.clone(),
                constraint,
            });
        }
    }
    pub(super) fn redo(&mut self) {
        self.framing.cancel_drag();
        self.adjustment_drag = None;
        if self.history.redo(&mut self.edits) {
            self.sync_framing_aspect();
            self.changed();
        }
    }
    pub(super) fn save(&mut self) {
        if matches!(self.busy, Some(Busy::Load(_))) || self.autosave.in_flight.is_some() {
            return;
        }
        if let Some(path) = &self.path {
            self.autosave.failed = false;
            self.autosave.in_flight = Some(self.edits.clone());
            self.status = "Saving edits…".into();
            self.send(Work::Save {
                load_id: self.load_generation,
                path: sidecar::path_for(path),
                edits: self.edits.clone(),
            });
        }
    }
    pub(super) fn generate(&mut self, gaps: bool, regenerate: bool) {
        if self.busy.is_some() {
            return;
        }
        self.finish_edit_gesture();
        #[cfg(feature = "moebius")]
        if let (Some(image), Some(path)) = (&self.image, &self.path) {
            let work = Work::Generate {
                load_id: self.load_generation,
                revision: self.history.revision(),
                path: path.clone(),
                image: image.clone(),
                edits: self.edits.clone(),
                dabs: self.mask.clone(),
                gaps,
                steps: self.ai_steps,
                seed: self.ai_seed,
                regenerate,
            };
            self.busy = Some(Busy::Generate(self.load_generation));
            self.status = "Generating local fill…".into();
            self.send(work);
        }
        #[cfg(not(feature = "moebius"))]
        let _ = (gaps, regenerate);
    }
    pub(super) fn export(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let (Some(image), Some(original)) = (&self.image, &self.path) else {
            return;
        };
        let default = original.file_stem().unwrap_or_default().to_string_lossy();
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export photograph")
            .set_file_name(format!("{default}.jpg"))
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("16-bit PNG", &["png"])
            .add_filter("16-bit TIFF", &["tif", "tiff"])
            .add_filter("Float EXR", &["exr"])
            .save_file()
        {
            let space = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exr"))
            {
                OutputSpace::LinearSrgb
            } else {
                self.space
            };
            let work = Work::Export {
                load_id: self.load_generation,
                original: original.clone(),
                path,
                image: image.clone(),
                edits: self.edits.clone(),
                space,
            };
            self.busy = Some(Busy::Export(self.load_generation));
            self.status = "Exporting full resolution…".into();
            self.send(work);
        }
    }
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Opened {
                    id,
                    path,
                    image,
                    edits,
                } if id == self.load_generation => {
                    self.path = Some(path);
                    self.image = Some(image);
                    self.edits = edits.clone();
                    self.saved = edits;
                    self.history = History::default();
                    self.autosave = autosave::Autosave::default();
                    self.texture = None;
                    self.hdr_texture = None;
                    self.histogram.clear();
                    self.zoom = 1.;
                    self.center = [0.5; 2];
                    self.clone_source = None;
                    self.brush_stroke = BrushStroke::default();
                    self.stroke_recorded = false;
                    self.mask.clear();
                    self.busy = None;
                    self.perspective_status = None;
                    self.calibration_status = None;
                    self.framing = framing::Framing::default();
                    self.sync_framing_aspect();
                    self.adjustment_drag = None;
                    self.calibration_box = None;
                    self.tool = Tool::View;
                    self.changed();
                    self.status = "Ready".into();
                }
                Reply::Preview {
                    id,
                    region,
                    size,
                    pixels,
                    histogram,
                    elapsed,
                    backend,
                    managed_display,
                    reconstruction_progress,
                } if id == self.generation => {
                    #[cfg(target_os = "macos")]
                    {
                        self.present_managed = managed_display;
                    }
                    #[cfg(not(target_os = "macos"))]
                    let _ = managed_display;
                    match pixels {
                        PreviewPixels::Sdr(rgba) => {
                            let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                            if let Some(texture) = &mut self.texture {
                                texture.set(image, egui::TextureOptions::LINEAR);
                            } else {
                                self.texture = Some(ctx.load_texture(
                                    "photograph",
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            self.hdr_texture = None;
                            self.texture_region = region;
                        }
                        PreviewPixels::Hdr(hdr) => {
                            let state = self.presentation.as_ref().unwrap();
                            match display::hdr::Texture::upload(&state.device, &state.queue, &hdr) {
                                Ok(texture) => {
                                    self.hdr_texture = Some(texture);
                                    self.texture = None;
                                    self.texture_region = region;
                                }
                                Err(error) => {
                                    self.error = Some(error.to_string());
                                }
                            }
                        }
                    }
                    self.histogram = histogram;
                    if self.busy.is_none() {
                        self.status = if let Some([done, total]) = reconstruction_progress {
                            if total == 0 {
                                "Preparing Joint AI · Standard preview".into()
                            } else {
                                format!("Preparing Joint AI {done}/{total} · Standard preview")
                            }
                        } else {
                            format!("Preview {:.0} ms · {backend}", elapsed * 1000.)
                        };
                    }
                }
                Reply::Perspective {
                    load_id,
                    revision,
                    previous,
                    report,
                    apply,
                } if load_id == self.load_generation => {
                    ErrorScope::Perspective(load_id).finish_busy(&mut self.busy);
                    if previous == self.edits && revision == self.history.revision() {
                        self.perspective_status = Some(report.reason.clone());
                        let aspect =
                            report.analysis_size[1] as f64 / report.analysis_size[0] as f64;
                        let guides: Vec<_> = report
                            .segments
                            .iter()
                            .enumerate()
                            .map(|(i, s)| {
                                let point = |p: [f64; 2]| {
                                    [(p[0] + 0.5) as f32, (p[1] / aspect + 0.5) as f32]
                                };
                                let vertical = report.vertical_inliers.contains(&i);
                                let horizontal = report.horizontal_inliers.contains(&i);
                                crate::edits::PerspectiveGuide {
                                    start: point(s.start),
                                    end: point(s.end),
                                    axis: if vertical
                                        || !horizontal
                                            && (s.end[1] - s.start[1]).abs()
                                                > (s.end[0] - s.start[0]).abs()
                                    {
                                        crate::edits::GuideAxis::Vertical
                                    } else {
                                        crate::edits::GuideAxis::Horizontal
                                    },
                                    enabled: vertical || horizontal,
                                }
                            })
                            .collect();
                        // Manual fitting preserves excluded lines and the user's explicit labels.
                        if report.mask_provider != "manual" {
                            self.edits.geometry.guides = guides;
                            self.framing.selected = None;
                        }
                        if apply && let Some(mut geometry) = report.solution {
                            geometry.guides = self.edits.geometry.guides.clone();
                            self.edits.geometry = geometry;
                            self.status = "Perspective applied; drag guides to refine it".into();
                        } else {
                            self.status = if apply {
                                "Review the detected guides before correcting perspective".into()
                            } else {
                                "Guides ready; review or draw lines, then apply correction".into()
                            };
                        }
                        self.remember(previous);
                        self.sync_framing_aspect();
                        self.set_tool(Tool::Perspective);
                    } else {
                        self.status =
                            "Perspective result discarded because the edits changed".into();
                    }
                }
                Reply::AutoCropped {
                    load_id,
                    revision,
                    previous,
                    crop,
                    aspect,
                } if load_id == self.load_generation => {
                    ErrorScope::AutoCrop(load_id).finish_busy(&mut self.busy);
                    if previous == self.edits && revision == self.history.revision() {
                        self.edits.geometry.crop = crop;
                        self.edits.geometry.crop_aspect = aspect;
                        self.sync_framing_aspect();
                        self.remember(previous);
                        self.set_tool(Tool::Crop);
                        self.status = "Crop fitted; drag its edges to adjust".into();
                    } else {
                        self.status = "Crop result discarded because the edits changed".into();
                    }
                }
                Reply::Calibrated {
                    load_id,
                    revision,
                    previous,
                    sample,
                } if load_id == self.load_generation => {
                    ErrorScope::Calibrate(load_id).finish_busy(&mut self.busy);
                    if previous == self.edits && revision == self.history.revision() {
                        self.edits.scene.illuminant =
                            crate::calibration::Illuminant::Custom { xy: sample.xy };
                        self.edits.scene.calibration = [1.; 3];
                        self.remember(previous);
                        self.calibration_status = Some(format!(
                            "Illuminant estimated from {} samples",
                            sample.pixels
                        ));
                        self.status = "Colour calibration ready; save edits to keep it".into();
                    } else {
                        self.status =
                            "Calibration result discarded because the edits or history changed"
                                .into();
                    }
                }
                Reply::Saved {
                    load_id,
                    path,
                    edits,
                } if load_id == self.load_generation
                    && self
                        .path
                        .as_ref()
                        .is_some_and(|p| sidecar::path_for(p) == path) =>
                {
                    self.saved = edits;
                    self.autosave.in_flight = None;
                    if self.busy.is_none() {
                        self.status = "Edits saved".into();
                    }
                    if self.close_after_save && !self.dirty() {
                        self.close_after_save = false;
                        self.perform_pending(ctx);
                    } else if self.close_after_save {
                        self.save();
                    }
                }
                #[cfg(feature = "moebius")]
                Reply::Generated {
                    load_id,
                    revision,
                    previous,
                    recipe_hash,
                    fills,
                    replace,
                } if load_id == self.load_generation => {
                    ErrorScope::Generate(load_id).finish_busy(&mut self.busy);
                    if revision == self.history.revision()
                        && *previous == self.edits
                        && crate::synthesis::recipe_hash(&self.edits).ok().as_ref()
                            == Some(&recipe_hash)
                    {
                        let empty = fills.is_empty();
                        if replace {
                            self.edits.display.synthesis.clear();
                        }
                        self.edits.display.synthesis.extend(fills);
                        self.remember(*previous);
                        self.mask.clear();
                        self.tool = Tool::View;
                        self.status = if empty && replace {
                            "No geometric gaps remain; save edits to keep the update".into()
                        } else if empty {
                            "No geometric gaps remain".into()
                        } else {
                            "Generated fill ready; save edits to keep it".into()
                        };
                    } else {
                        self.status =
                            "Generated result discarded because the edits or history changed"
                                .into();
                    }
                }
                Reply::Exported { load_id, path } if self.busy == Some(Busy::Export(load_id)) => {
                    self.busy = None;
                    self.status = format!(
                        "Exported {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                Reply::Error { scope, message }
                    if scope.applies(self.load_generation, self.generation) =>
                {
                    self.error = Some(message);
                    scope.finish_busy(&mut self.busy);
                    if self.busy.is_none() {
                        self.status = match scope {
                            ErrorScope::Load(_) => "Could not open photo",
                            ErrorScope::Preview(_) => "Could not render preview",
                            ErrorScope::Save(_) => "Could not save edits",
                            ErrorScope::Export(_) => "Could not export photo",
                            ErrorScope::Perspective(_) => "Could not analyze perspective",
                            ErrorScope::Calibrate(_) => "Could not estimate illuminant",
                            ErrorScope::AutoCrop(_) => "Could not fit a crop",
                            #[cfg(feature = "moebius")]
                            ErrorScope::Generate(_) => "Could not generate fill",
                        }
                        .into();
                    }
                    if matches!(scope, ErrorScope::Save(id) if id == self.load_generation) {
                        self.close_after_save = false;
                        self.autosave.in_flight = None;
                        self.autosave.failed = true;
                    }
                }
                _ => {}
            }
        }
    }
    pub(super) fn perform_pending(&mut self, ctx: &egui::Context) {
        self.close_after_save = false;
        match self.pending.take() {
            Some(Pending::Close) => {
                self.autosave.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(Pending::Open(path)) => self.open(path),
            None => {}
        }
    }

    pub(super) fn cancel_pending(&mut self) {
        self.pending = None;
        self.close_after_save = false;
    }
}
