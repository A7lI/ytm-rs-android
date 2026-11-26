// User commands and worker results remain actionable while the Activity is paused.
pub(super) fn has_work(resumed: bool, redraw: bool, user_event: bool) -> bool {
    user_event || (resumed && redraw)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_play_and_pause_commands_wake_without_a_surface() {
        assert!(has_work(false, false, true));
        assert!(has_work(false, true, true));
    }
    #[test]
    fn paused_render_requests_sleep_until_resumed() {
        assert!(!has_work(false, true, false));
        assert!(!has_work(false, false, false));
        assert!(has_work(true, true, false));
        assert!(!has_work(true, false, false));
    }
}
