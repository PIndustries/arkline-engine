//! Capture scheduling only: cached pixels and their actual capture times belong
//! to the renderer. Every callback must supply the complete set of feeds that
//! will actually be visible in this UI frame, including cold caches.

#[derive(Clone, Copy, Debug)]
pub(crate) struct FeedCaptureCandidate {
    pub(crate) key: u64,
    pub(crate) painted_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct FeedCaptureBudget {
    ui_frame: Option<u64>,
    winner: Option<u64>,
    claimed: bool,
    last_captured_key: Option<u64>,
}

impl FeedCaptureBudget {
    /// Claim this frame's capture, at most once, even for a single feed.
    ///
    /// `ui_frame` must be a presentation identity (egui's cumulative frame
    /// number), never the simulation clock: paused worlds still present frames.
    /// The first callback freezes a winner from all visible candidates. Later
    /// callbacks cannot change it by reordering candidates or updating caches.
    /// Callers should only claim immediately before an actual capture.
    pub(crate) fn should_capture(
        &mut self,
        ui_frame: u64,
        now_ms: u64,
        key: u64,
        visible: &[FeedCaptureCandidate],
    ) -> bool {
        if self.ui_frame != Some(ui_frame) {
            self.ui_frame = Some(ui_frame);
            self.claimed = false;
            self.winner = self.choose(now_ms, visible);
        }
        if !self.claimed && self.winner == Some(key) {
            self.claimed = true;
            self.last_captured_key = Some(key);
            true
        } else {
            false
        }
    }

    fn choose(&self, now_ms: u64, visible: &[FeedCaptureCandidate]) -> Option<u64> {
        let interval_ms = if visible.len() > 1 { 33 } else { 16 };
        let eligible = || {
            visible.iter().filter(|candidate| {
                candidate.painted_ms.is_none_or(|before| {
                    now_ms < before || now_ms.saturating_sub(before) >= interval_ms
                })
            })
        };
        // Stable key order makes fairness independent of callback/tile order.
        // A removed key still supplies a valid cursor into the remaining set;
        // cold page changes naturally wrap to their first eligible key.
        self.last_captured_key
            .and_then(|last| {
                eligible()
                    .map(|candidate| candidate.key)
                    .filter(|key| *key > last)
                    .min()
            })
            .or_else(|| eligible().map(|candidate| candidate.key).min())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(key: u64, painted_ms: Option<u64>) -> FeedCaptureCandidate {
        FeedCaptureCandidate { key, painted_ms }
    }

    #[test]
    fn cold_page_has_one_capture_per_frame_and_fills_without_starvation() {
        let mut budget = FeedCaptureBudget::default();
        let mut visible = [candidate(40, None), candidate(10, None), candidate(30, None), candidate(20, None)];
        for (frame, expected) in [(1, 10), (2, 20), (3, 30), (4, 40)] {
            let mut captured = Vec::new();
            for feed in visible {
                if budget.should_capture(frame, 100, feed.key, &visible) {
                    captured.push(feed.key);
                }
            }
            assert_eq!(captured, [expected]);
            assert!(!budget.should_capture(frame, 100, expected, &visible));
            visible.iter_mut().find(|feed| feed.key == expected).unwrap().painted_ms = Some(100);
        }
    }

    #[test]
    fn warm_deadlines_and_single_feed_duplicate_guard() {
        let mut budget = FeedCaptureBudget::default();
        let single = [candidate(7, Some(100))];
        assert!(!budget.should_capture(1, 115, 7, &single));
        assert!(budget.should_capture(2, 116, 7, &single));
        assert!(!budget.should_capture(2, 200, 7, &single));
        let multiple = [candidate(7, Some(100)), candidate(8, Some(100))];
        assert!(!budget.should_capture(3, 132, 8, &multiple));
        assert!(budget.should_capture(4, 133, 8, &multiple));
        assert!(!budget.should_capture(4, 133, 7, &multiple));
    }

    #[test]
    fn winner_is_frozen_and_reordering_does_not_change_rotation() {
        let mut budget = FeedCaptureBudget::default();
        let visible = [candidate(2, None), candidate(1, None), candidate(3, None)];
        assert!(!budget.should_capture(10, 100, 3, &visible));
        let reordered = [visible[2], visible[0], visible[1]];
        assert!(!budget.should_capture(10, 100, 2, &reordered));
        assert!(budget.should_capture(10, 100, 1, &reordered));
        assert!(budget.should_capture(11, 100, 2, &reordered));
        assert!(budget.should_capture(12, 100, 3, &visible));
    }

    #[test]
    fn paused_clock_new_pages_and_clock_reset_remain_bounded() {
        let mut budget = FeedCaptureBudget::default();
        let visible = [candidate(2, Some(500)), candidate(4, Some(500))];
        assert!(budget.should_capture(90, 20, 2, &visible));
        assert!(!budget.should_capture(90, 19, 4, &visible));
        assert!(budget.should_capture(91, 20, 4, &visible));
        let page = [candidate(1, None), candidate(3, None)];
        assert!(budget.should_capture(92, 20, 1, &page));
        assert!(budget.should_capture(93, 20, 3, &page));
        assert!(!budget.should_capture(94, 20, 3, &[]));
        assert!(budget.should_capture(0, 20, 1, &page));
    }

    #[test]
    fn warm_feed_gets_turn_despite_new_cold_feeds() {
        let mut budget = FeedCaptureBudget::default();
        assert!(budget.should_capture(1, 100, 1, &[candidate(1, None), candidate(2, Some(0))]));
        assert!(budget.should_capture(2, 100, 2, &[candidate(0, None), candidate(2, Some(0))]));
    }
}
