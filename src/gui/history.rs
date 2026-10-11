//! Bounded document history, separate from preview/view revisions.
use super::Edits;

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Edits>,
    redo: Vec<Edits>,
    gesture: Option<Edits>,
    revision: u64,
}
impl History {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.gesture.is_some()
    }
    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    #[cfg(test)]
    pub(super) fn undo_len(&self) -> usize {
        self.undo.len()
    }
    fn push(&mut self, previous: Edits) {
        self.undo.push(previous);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
    }
    pub(super) fn record(&mut self, previous: Edits, current: &Edits) -> bool {
        if previous == *current {
            return false;
        }
        self.push(previous);
        true
    }
    pub(super) fn begin_gesture(&mut self, previous: Edits) {
        if self.gesture.is_none() {
            self.gesture = Some(previous);
            self.redo.clear();
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub(super) fn finish_gesture(&mut self, current: &Edits) -> bool {
        self.gesture
            .take()
            .is_some_and(|previous| self.record(previous, current))
    }
    pub(super) fn begin_stroke(&mut self, previous: Edits) {
        self.push(previous);
    }
    pub(super) fn undo(&mut self, current: &mut Edits) -> bool {
        self.finish_gesture(current);
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(current, previous));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
    pub(super) fn redo(&mut self, current: &mut Edits) -> bool {
        self.finish_gesture(current);
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(current, next));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
}
