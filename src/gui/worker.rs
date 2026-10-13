//! Photo-worker protocol and background rendering, inference and durable I/O.
use super::*;

#[derive(Clone)]
pub(super) enum Work {
    Open {
        id: u64,
        path: PathBuf,
    },
    Render {
        id: u64,
        image: Arc<SensorImage>,
        edits: Edits,
        region: [f32; 4],
        size: [usize; 2],
        profile: Option<display::Icc>,
        hdr: bool,
    },
    Save {
        load_id: u64,
        path: PathBuf,
        edits: Edits,
    },
    Perspective {
        load_id: u64,
        revision: u64,
        image: Arc<SensorImage>,
        edits: Edits,
        mode: crate::perspective::Mode,
        provider: crate::perspective::MaskProvider,
        manual: bool,
        apply: bool,
        crop: Option<crate::crop::Constraint>,
    },
    AutoCrop {
        load_id: u64,
        revision: u64,
        image: Arc<SensorImage>,
        edits: Edits,
        constraint: crate::crop::Constraint,
    },
    Calibrate {
        load_id: u64,
        revision: u64,
        image: Arc<SensorImage>,
        edits: Edits,
        region: [f32; 4],
    },
    #[cfg(feature = "moebius")]
    Generate {
        load_id: u64,
        revision: u64,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
        dabs: Vec<crate::synthesis::MaskDab>,
        gaps: bool,
        steps: usize,
        seed: i64,
        regenerate: bool,
    },
    Export {
        load_id: u64,
        original: PathBuf,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
        space: OutputSpace,
    },
}
pub(super) enum Reply {
    Opened {
        id: u64,
        path: PathBuf,
        image: Arc<SensorImage>,
        edits: Edits,
    },
    Preview {
        id: u64,
        region: [f32; 4],
        size: [usize; 2],
        pixels: PreviewPixels,
        histogram: Vec<f32>,
        elapsed: f64,
        backend: String,
        managed_display: bool,
        reconstruction_progress: Option<[usize; 2]>,
    },
    Saved {
        load_id: u64,
        path: PathBuf,
        edits: Edits,
    },
    Perspective {
        load_id: u64,
        revision: u64,
        previous: Edits,
        report: Box<crate::perspective::Report>,
        apply: bool,
    },
    AutoCropped {
        load_id: u64,
        revision: u64,
        previous: Edits,
        crop: [f32; 4],
        aspect: Option<f32>,
    },
    Calibrated {
        load_id: u64,
        revision: u64,
        previous: Edits,
        sample: crate::calibration::Sample,
    },
    #[cfg(feature = "moebius")]
    Generated {
        load_id: u64,
        revision: u64,
        previous: Box<Edits>,
        recipe_hash: String,
        fills: Vec<crate::synthesis::GeneratedFill>,
        replace: bool,
    },
    Exported {
        load_id: u64,
        path: PathBuf,
    },
    Error {
        scope: ErrorScope,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ErrorScope {
    Load(u64),
    Preview(u64),
    Save(u64),
    Export(u64),
    Perspective(u64),
    Calibrate(u64),
    AutoCrop(u64),
    #[cfg(feature = "moebius")]
    Generate(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Busy {
    Load(u64),
    Export(u64),
    Perspective(u64),
    Calibrate(u64),
    AutoCrop(u64),
    #[cfg(feature = "moebius")]
    Generate(u64),
}
impl ErrorScope {
    pub(super) fn applies(self, load: u64, preview: u64) -> bool {
        match self {
            Self::Load(id) => id == load,
            Self::Preview(id) => id == preview,
            Self::Perspective(id) => id == load,
            Self::Calibrate(id) => id == load,
            Self::AutoCrop(id) => id == load,
            // Durable I/O failures still need to name the failed destination,
            // even after the user changes photographs.
            Self::Save(_) | Self::Export(_) => true,
            #[cfg(feature = "moebius")]
            Self::Generate(id) => id == load,
        }
    }
    pub(super) fn finish_busy(self, busy: &mut Option<Busy>) {
        let finished = match self {
            Self::Load(id) => Some(Busy::Load(id)),
            Self::Export(id) => Some(Busy::Export(id)),
            Self::Perspective(id) => Some(Busy::Perspective(id)),
            Self::Calibrate(id) => Some(Busy::Calibrate(id)),
            Self::AutoCrop(id) => Some(Busy::AutoCrop(id)),
            #[cfg(feature = "moebius")]
            Self::Generate(id) => Some(Busy::Generate(id)),
            Self::Preview(_) | Self::Save(_) => None,
        };
        if finished.is_some() && *busy == finished {
            *busy = None;
        }
    }
}
impl Work {
    fn error_scope(&self) -> ErrorScope {
        match self {
            Self::Open { id, .. } => ErrorScope::Load(*id),
            Self::Render { id, .. } => ErrorScope::Preview(*id),
            Self::Perspective { load_id, .. } => ErrorScope::Perspective(*load_id),
            Self::Calibrate { load_id, .. } => ErrorScope::Calibrate(*load_id),
            Self::AutoCrop { load_id, .. } => ErrorScope::AutoCrop(*load_id),
            #[cfg(feature = "moebius")]
            Self::Generate { load_id, .. } => ErrorScope::Generate(*load_id),
            Self::Save { load_id, .. } => ErrorScope::Save(*load_id),
            Self::Export { load_id, .. } => ErrorScope::Export(*load_id),
        }
    }
}

pub(super) enum PreviewPixels {
    Sdr(Vec<u8>),
    Hdr(display::hdr::Frame),
}

pub(super) fn worker(
    rx: mpsc::Receiver<Work>,
    tx: mpsc::Sender<Reply>,
    ctx: egui::Context,
    backend: Backend,
) {
    let mut renderer = Renderer::new(backend);
    let mut perspective = crate::perspective::Engine::default();
    let mut display_encoder = display::Encoder::default();
    let mut last_preview = None;
    loop {
        let mut work = if renderer.reconstruction_pending() {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(work) => work,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let Some(work) = last_preview.clone() else {
                        continue;
                    };
                    work
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(work) => work,
                Err(_) => break,
            }
        };
        // Coalesce only preview requests; durable save/export operations are always executed.
        while matches!(work, Work::Render { .. }) {
            match rx.try_recv() {
                Ok(next) => work = next,
                Err(_) => break,
            }
        }
        if matches!(work, Work::Render { .. }) {
            last_preview = Some(work.clone());
        }
        let scope = work.error_scope();
        let destination = match &work {
            Work::Save { path, .. } => Some(format!("Saving edits to {}", path.display())),
            Work::Export { path, .. } => Some(format!("Exporting {}", path.display())),
            _ => None,
        };
        let result: Result<Reply> = (|| match work {
            Work::Open { id, path } => {
                renderer.cancel_reconstruction();
                last_preview = None;
                let image = Arc::new(SensorImage::open(&path)?);
                let edits = sidecar::load_for_default(&path, Edits::for_image(&image))?;
                renderer.set_document(path.clone());
                Ok(Reply::Opened {
                    id,
                    path,
                    image,
                    edits,
                })
            }
            Work::Render {
                id,
                image,
                edits,
                region,
                size,
                profile,
                hdr,
            } => {
                let start = Instant::now();
                let mut preview_edits = edits.clone();
                let hash = crate::synthesis::recipe_hash(&edits)?;
                preview_edits.display.synthesis.retain(|fill| {
                    fill.recipe_sha256 == hash
                        && fill.source_color_revision == image.metadata.color_revision
                });
                let (rendered, reconstruction_progress) = renderer.render_preview_region(
                    image,
                    &preview_edits,
                    region,
                    size[0],
                    size[1],
                )?;
                let mut histogram = vec![0f32; 128];
                for p in &rendered.pixels {
                    if p[3] < 0.5 {
                        continue;
                    }
                    let l = color::srgb_encode(color::luminance([p[0], p[1], p[2]]));
                    histogram[(l.clamp(0., 1.) * 127.) as usize] += 1.;
                }
                let peak = histogram.iter().copied().fold(1., f32::max);
                for v in &mut histogram {
                    *v = (*v / peak).sqrt();
                }
                let pixels = if hdr {
                    ensure!(
                        profile.is_none(),
                        "HDR previews require automatic compositor color management"
                    );
                    PreviewPixels::Hdr(display::hdr::Frame::from_rendered(&rendered, 1.)?)
                } else {
                    PreviewPixels::Sdr(display_encoder.encode(&rendered, profile.as_ref())?)
                };
                Ok(Reply::Preview {
                    id,
                    region: [
                        edits.geometry.crop[0] + region[0] * edits.geometry.crop[2],
                        edits.geometry.crop[1] + region[1] * edits.geometry.crop[3],
                        region[2] * edits.geometry.crop[2],
                        region[3] * edits.geometry.crop[3],
                    ],
                    size,
                    pixels,
                    histogram,
                    elapsed: start.elapsed().as_secs_f64(),
                    backend: renderer.label().into(),
                    managed_display: profile.is_none(),
                    reconstruction_progress,
                })
            }
            Work::Perspective {
                load_id,
                revision,
                image,
                edits,
                mode,
                provider,
                manual,
                apply,
                crop,
            } => {
                let mut report = if manual {
                    crate::perspective::fit_guides(
                        &edits.geometry.guides,
                        [image.metadata.width, image.metadata.height],
                        &edits.geometry,
                        mode,
                    )?
                } else {
                    perspective.run(&mut renderer, image.clone(), &edits, mode, provider)?
                };
                if apply && let (Some(geometry), Some(constraint)) = (&mut report.solution, crop) {
                    let mut corrected = edits.clone();
                    corrected.geometry = geometry.clone();
                    geometry.crop = crate::crop::largest(&image, &corrected, constraint)?;
                    geometry.crop_aspect = match constraint {
                        crate::crop::Constraint::LargestArea => None,
                        crate::crop::Constraint::Aspect(r) => Some(r),
                    };
                }
                Ok(Reply::Perspective {
                    load_id,
                    revision,
                    previous: edits,
                    report: Box::new(report),
                    apply,
                })
            }
            Work::AutoCrop {
                load_id,
                revision,
                image,
                edits,
                constraint,
            } => {
                let crop = crate::crop::largest(&image, &edits, constraint)?;
                Ok(Reply::AutoCropped {
                    load_id,
                    revision,
                    previous: edits,
                    crop,
                    aspect: match constraint {
                        crate::crop::Constraint::LargestArea => None,
                        crate::crop::Constraint::Aspect(r) => Some(r),
                    },
                })
            }
            Work::Calibrate {
                load_id,
                revision,
                image,
                edits,
                region,
            } => {
                let sample = crate::calibration::estimate(&image, &edits, region)?;
                Ok(Reply::Calibrated {
                    load_id,
                    revision,
                    previous: edits,
                    sample,
                })
            }
            Work::Save {
                load_id,
                path,
                edits,
            } => {
                sidecar::save(&path, &edits)?;
                Ok(Reply::Saved {
                    load_id,
                    path,
                    edits,
                })
            }
            #[cfg(feature = "moebius")]
            Work::Generate {
                load_id,
                revision,
                path,
                image,
                edits,
                dabs,
                gaps,
                steps,
                seed,
                regenerate,
            } => {
                renderer.set_document(path);
                let hash = crate::synthesis::recipe_hash(&edits)?;
                let (w, h) = renderer.dimensions(image.clone(), &edits, None)?;
                let wants_gaps =
                    gaps || regenerate && edits.display.synthesis.iter().any(|fill| fill.fill_gaps);
                let gap_regions = if wants_gaps {
                    let mut base = edits.clone();
                    base.display.synthesis.clear();
                    renderer.gap_contexts(image.clone(), &base)?
                } else {
                    vec![]
                };
                let jobs = if regenerate {
                    crate::synthesis::regeneration_contexts(
                        &edits.display.synthesis,
                        w,
                        h,
                        &gap_regions,
                    )?
                } else if gaps {
                    gap_regions
                        .into_iter()
                        .map(|region| crate::synthesis::FillContext {
                            region,
                            dabs: vec![],
                            fill_gaps: true,
                        })
                        .collect()
                } else {
                    vec![crate::synthesis::FillContext {
                        region: crate::synthesis::brush_context(&dabs, w, h)?,
                        dabs,
                        fill_gaps: false,
                    }]
                };
                if !regenerate {
                    ensure!(!jobs.is_empty(), "No geometric gaps to fill");
                }
                let mut fills = Vec::new();
                let mut current = edits.clone();
                if regenerate {
                    current.display.synthesis.clear();
                }
                for job in jobs {
                    let settings = crate::moebius::Sampling {
                        steps,
                        seed,
                        ..Default::default()
                    };
                    if let Some(fill) = renderer.generate_fill(
                        image.clone(),
                        &current,
                        job.region,
                        job.dabs,
                        job.fill_gaps,
                        &settings,
                    )? {
                        current.display.synthesis.push(fill.clone());
                        fills.push(fill);
                    }
                    ctx.request_repaint();
                }
                Ok(Reply::Generated {
                    load_id,
                    revision,
                    previous: Box::new(edits),
                    recipe_hash: hash,
                    fills,
                    replace: regenerate,
                })
            }
            Work::Export {
                load_id,
                original,
                path,
                image,
                edits,
                space,
            } => {
                ensure!(
                    path.canonicalize().ok().as_ref() != Some(&original.canonicalize()?),
                    "Export cannot overwrite the original"
                );
                let rendered = renderer.render(image, &edits, None)?;
                export::write(&path, &rendered, space, true)?;
                Ok(Reply::Exported { load_id, path })
            }
        })();
        let reply = result.unwrap_or_else(|e| {
            let error = if let Some(destination) = destination {
                e.context(destination)
            } else {
                e
            };
            Reply::Error {
                scope,
                message: format!("{error:#}"),
            }
        });
        if tx.send(reply).is_err() {
            break;
        }
        ctx.request_repaint();
    }
}
