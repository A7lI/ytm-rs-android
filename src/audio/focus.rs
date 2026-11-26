//! Playback intent across Android audio-focus changes.

#[derive(Clone, Copy)]
pub(crate) enum Change {
    TemporaryLoss,
    Gain,
    PermanentLoss,
}

pub(super) enum Effect {
    Pause,
    Resume,
}

#[derive(Default)]
pub(super) struct Focus {
    suspended: bool,
    resume: bool,
    paused_generation: Option<u64>,
}

impl Focus {
    pub(super) fn change(&mut self, change: Change, playing: bool, generation: u64) -> Option<Effect> {
        match change {
            Change::TemporaryLoss => {
                self.suspended = true;
                self.resume |= playing;
                Some(Effect::Pause)
            }
            Change::Gain => {
                self.suspended = false;
                if std::mem::take(&mut self.resume) { Some(Effect::Resume) } else { None }
            }
            Change::PermanentLoss => {
                self.suspended = false;
                self.pause(generation);
                Some(Effect::Pause)
            }
        }
    }

    pub(super) fn pause(&mut self, generation: u64) {
        self.resume = false;
        self.paused_generation = Some(generation);
    }

    pub(super) fn stop(&mut self) {
        self.resume = false;
    }

    pub(super) fn start(&mut self, generation: u64) -> bool {
        if self.paused_generation == Some(generation) {
            return false;
        }
        self.play()
    }

    pub(super) fn user_play(&mut self) -> bool {
        self.paused_generation = None;
        self.play()
    }

    fn play(&mut self) -> bool {
        if self.suspended {
            self.resume = true;
        }
        !self.suspended
    }

    pub(super) fn will_resume(&self) -> bool {
        self.suspended && self.resume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_loss_preserves_playback_and_repeated_loss_does_not_cancel_it() {
        let mut focus = Focus::default();
        assert!(matches!(focus.change(Change::TemporaryLoss, true, 1), Some(Effect::Pause)));
        focus.change(Change::TemporaryLoss, false, 1);
        assert!(focus.will_resume());
        assert!(matches!(focus.change(Change::Gain, false, 1), Some(Effect::Resume)));
        assert!(!focus.will_resume());
        assert!(focus.change(Change::Gain, true, 1).is_none());
    }

    #[test]
    fn manual_pause_and_permanent_loss_cancel_automatic_resume() {
        let mut focus = Focus::default();
        focus.change(Change::TemporaryLoss, true, 1);
        focus.pause(1);
        assert!(focus.change(Change::Gain, false, 1).is_none());
        assert!(!focus.start(1));
        assert!(focus.user_play());
        focus.change(Change::TemporaryLoss, true, 1);
        focus.change(Change::PermanentLoss, false, 1);
        assert!(focus.change(Change::Gain, false, 1).is_none());
        assert!(focus.start(2));
    }

    #[test]
    fn playback_that_finishes_buffering_during_a_call_waits_for_focus() {
        let mut focus = Focus::default();
        focus.change(Change::TemporaryLoss, false, 1);
        assert!(!focus.start(1));
        assert!(focus.will_resume());
        assert!(matches!(focus.change(Change::Gain, false, 1), Some(Effect::Resume)));
    }

    #[test]
    fn stopping_while_interrupted_cancels_resume() {
        let mut focus = Focus::default();
        focus.change(Change::TemporaryLoss, true, 1);
        focus.stop();
        assert!(focus.change(Change::Gain, false, 1).is_none());
    }

    #[test]
    fn idle_player_does_not_start_from_a_focus_change() {
        let mut focus = Focus::default();
        focus.change(Change::TemporaryLoss, false, 1);
        assert!(focus.change(Change::Gain, false, 1).is_none());
    }
}
