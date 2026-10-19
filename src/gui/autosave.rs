//! Debounced snapshots and a serial I/O lane independent of photo processing.
use super::*;

#[derive(Default)]
pub(super) struct Autosave {
    observed: Option<Edits>,
    due: f64,
    pub in_flight: Option<Edits>,
    pub failed: bool,
    pub closing: bool,
}

impl Editor {
    pub(super) fn autosave_tick(&mut self, ctx: &egui::Context) {
        if self.path.is_none() || self.image.is_none() || matches!(self.busy, Some(Busy::Load(_))) {
            return;
        }
        let now = ctx.input(|i| i.time);
        if self.autosave.observed.as_ref() != Some(&self.edits) {
            self.autosave.observed = Some(self.edits.clone());
            self.autosave.due = now + 0.4;
        }
        if !self.dirty() || self.autosave.failed {
            return;
        }
        if self.autosave.in_flight.is_none() && (self.close_after_save || now >= self.autosave.due)
        {
            self.save();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

pub(super) fn save_worker(rx: mpsc::Receiver<Work>, tx: mpsc::Sender<Reply>, ctx: egui::Context) {
    for work in rx {
        let Work::Save {
            load_id,
            path,
            edits,
        } = work
        else {
            continue;
        };
        let reply = match sidecar::save(&path, &edits) {
            Ok(()) => Reply::Saved {
                load_id,
                path,
                edits,
            },
            Err(error) => Reply::Error {
                scope: ErrorScope::Save(load_id),
                message: format!("Saving {}: {error:#}", path.display()),
            },
        };
        if tx.send(reply).is_err() {
            break;
        }
        ctx.request_repaint();
    }
}
