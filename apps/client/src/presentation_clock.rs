//! Proposed display-only clock; runtime freshness separation is not integrated.
//!
//! Authoritative pause, step, seek and session changes must call `reset` (or
//! `clear`), rather than being inferred from timestamp differences. Snapshot
//! timestamps and receipt freshness remain owned by the caller.

const PREDICTION_LIMIT_MS: u64 = 350;
const CORRECTION_HORIZON_MS: i128 = 200;
const NORMAL_RATE: i128 = 1_000;
const MAX_RATE_CORRECTION: i128 = 100;

#[derive(Default)]
pub(crate) struct PresentationClock {
    anchor: Option<Anchor>,
}

struct Anchor {
    local_ms: u64,
    presentation_us: u128,
    latest_authority_ms: u64,
    // Microseconds of presentation advancement per local millisecond.
    rate: u128,
}

impl PresentationClock {
    pub(crate) fn clear(&mut self) {
        self.anchor = None;
    }

    /// Explicit discontinuity: permits exact authoritative backwards changes.
    /// The caller holds this value while paused; rendering alone is running.
    pub(crate) fn reset(&mut self, authoritative_ms: u64, local_ms: u64) {
        self.anchor = Some(Anchor {
            local_ms,
            presentation_us: u128::from(authoritative_ms) * 1_000,
            latest_authority_ms: authoritative_ms,
            rate: NORMAL_RATE as u128,
        });
    }

    /// Ordinary receipt: preserve presentation continuity and adjust its rate.
    /// `local_ms` is an elapsed monotonic local clock, independent of authority.
    pub(crate) fn observe_running(&mut self, authoritative_ms: u64, local_ms: u64) {
        let Some(anchor) = self.anchor.as_ref() else {
            self.reset(authoritative_ms, local_ms);
            return;
        };
        // Saturation also makes an accidentally older local receipt harmless.
        let local_ms = local_ms.max(anchor.local_ms);
        let presentation_us = anchor.render_us(local_ms);
        let latest_authority_ms = authoritative_ms.max(anchor.latest_authority_ms);
        let error_us = (u128::from(latest_authority_ms) * 1_000) as i128 - presentation_us as i128;
        let correction =
            (error_us / CORRECTION_HORIZON_MS).clamp(-MAX_RATE_CORRECTION, MAX_RATE_CORRECTION);
        self.anchor = Some(Anchor {
            local_ms,
            presentation_us,
            latest_authority_ms,
            rate: (NORMAL_RATE + correction) as u128,
        });
    }

    pub(crate) fn render_at(&self, local_ms: u64) -> Option<u64> {
        self.anchor
            .as_ref()
            .map(|anchor| (anchor.render_us(local_ms) / 1_000) as u64)
    }
}

impl Anchor {
    fn render_us(&self, local_ms: u64) -> u128 {
        let elapsed_ms = u128::from(local_ms.saturating_sub(self.local_ms));
        let cap_us =
            u128::from(self.latest_authority_ms.saturating_add(PREDICTION_LIMIT_MS)) * 1_000;
        (self.presentation_us + elapsed_ms * self.rate).min(cap_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_jitter_exposes_old_rebases_and_preserves_proposed_continuity() {
        // Existing embedded formula: latest authority + elapsed since receipt.
        let old_before_first_receipt = 1_000 + 120;
        let old_after_first_receipt = 1_100;
        let old_before_second_receipt = 1_100 + (200 - 120);
        let old_after_second_receipt = 1_200;
        assert_eq!(old_before_first_receipt - old_after_first_receipt, 20);
        assert_eq!(old_after_second_receipt - old_before_second_receipt, 20);

        let mut clock = PresentationClock::default();
        clock.observe_running(1_000, 0);
        assert_eq!(clock.render_at(119), Some(1_119));
        assert_eq!(clock.render_at(120), Some(1_120));
        clock.observe_running(1_100, 120);
        assert_eq!(clock.render_at(120), Some(1_120));
        assert_eq!(clock.render_at(199), Some(1_191));
        assert_eq!(clock.render_at(200), Some(1_192));
        clock.observe_running(1_200, 200);
        assert_eq!(clock.render_at(200), Some(1_192));
        assert_eq!(clock.render_at(210), Some(1_202));
    }

    #[test]
    fn backwards_authority_receipt_does_not_rewind_or_lower_prediction_cap() {
        let mut clock = PresentationClock::default();
        clock.reset(1_000, 0);
        clock.observe_running(1_100, 100);
        clock.observe_running(900, 120);
        assert_eq!(clock.render_at(120), Some(1_120));
        assert_eq!(clock.render_at(1_000), Some(1_450));
    }

    #[test]
    fn packet_silence_stops_prediction_after_350_ms() {
        let mut clock = PresentationClock::default();
        clock.reset(1_000, 0);
        assert_eq!(clock.render_at(349), Some(1_349));
        assert_eq!(clock.render_at(350), Some(1_350));
        assert_eq!(clock.render_at(60_000), Some(1_350));
        assert_eq!(clock.render_at(u64::MAX), Some(1_350));
    }

    #[test]
    fn large_gap_keeps_receipt_continuous_and_limits_catchup_rate() {
        let mut clock = PresentationClock::default();
        clock.reset(1_000, 0);
        clock.observe_running(100_000, 60_000);
        assert_eq!(clock.render_at(60_000), Some(1_350));
        assert_eq!(clock.render_at(60_100), Some(1_460));
        // A large deliberate seek needs reset; ordinary receipt catchup is slow.
        clock.reset(100_000, 60_100);
        assert_eq!(clock.render_at(60_100), Some(100_000));
    }

    #[test]
    fn explicit_pause_step_and_session_reset_accept_exact_authority() {
        let mut clock = PresentationClock::default();
        clock.reset(1_000, 0);
        assert_eq!(clock.render_at(120), Some(1_120));
        clock.reset(1_100, 120); // Authoritative pause; caller holds frozen value.
        assert_eq!(clock.render_at(120), Some(1_100));
        clock.reset(1_116, 1_000); // One authoritative step while paused.
        assert_eq!(clock.render_at(1_000), Some(1_116));
        clock.reset(1_116, 5_000); // Resume starts at the authoritative position.
        assert_eq!(clock.render_at(5_010), Some(1_126));
        clock.clear();
        assert_eq!(clock.render_at(9_000), None);
        clock.observe_running(500, 9_000); // A different session may start earlier.
        assert_eq!(clock.render_at(9_000), Some(500));
    }
}
