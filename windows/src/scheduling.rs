use std::time::{Duration, Instant};
/// Single-window reactive scheduling. Hidden windows retain dirtiness and drop
/// cosmetic deadlines; OS exposes can present the retained bitmap without layout.
pub struct FrameSchedule {
    pub visible: bool,
    pub dirty: bool,
    deadline: Option<Instant>,
}
impl Default for FrameSchedule {
    fn default() -> Self {
        Self {
            visible: true,
            dirty: true,
            deadline: None,
        }
    }
}
impl FrameSchedule {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn visibility(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.dirty = true;
            self.deadline = None;
        }
    }
    pub fn completed(&mut self, now: Instant, delay: Duration) {
        self.dirty = false;
        self.deadline = if self.visible && delay != Duration::MAX {
            now.checked_add(delay)
        } else {
            None
        };
    }
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.visible && self.deadline.is_some_and(|d| d <= now) {
            self.deadline = None;
            self.dirty = true;
        }
        self.visible && self.dirty
    }
    pub fn deadline(&self) -> Option<Instant> {
        if self.visible { self.deadline } else { None }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_animation_settles_and_hidden_work_does_not_render() {
        let now = Instant::now();
        let mut s = FrameSchedule::default();
        assert!(s.take_due(now));
        s.completed(now, Duration::from_millis(16));
        assert!(!s.take_due(now));
        assert!(s.take_due(now + Duration::from_millis(16)));
        s.completed(now + Duration::from_millis(16), Duration::MAX);
        assert!(!s.take_due(now + Duration::from_secs(100)));
        assert!(s.deadline().is_none());
        s.visibility(false);
        s.invalidate();
        assert!(!s.take_due(now));
        assert!(s.deadline().is_none());
        s.visibility(true);
        assert!(s.take_due(now));
    }
}
