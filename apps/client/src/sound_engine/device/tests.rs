//! Device lifecycle/recovery tests; real output probes remain explicitly ignored.
use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audio_recovery_budget_is_capped_and_manual_retry_resets_it() {
    let mut budget = RecoveryBudget::default();
    assert_eq!(
        (0..5)
            .map(|_| budget.next_delay().unwrap().as_secs())
            .collect::<Vec<_>>(),
        [1, 2, 4, 8, 16]
    );
    assert!(budget.next_delay().is_none());
    budget.reset();
    assert_eq!(budget.next_delay().unwrap().as_secs(), 1);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audio_recovery_drops_failed_stream_before_reopen_and_discards_test() {
    struct Held {
        generation: u32,
        events: Arc<Mutex<Vec<String>>>,
        _cues: Receiver<Cue>,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            self.events
                .lock()
                .unwrap()
                .push(format!("drop{}", self.generation));
        }
    }
    let mut engine = SoundEngine::default();
    let mix = Mix {
        master: 0.7,
        effects: 0.3,
        muted: true,
        ducked: true,
        ..Mix::default()
    };
    engine.set_mix(mix);
    let (sender, initial) = mpsc::sync_channel(32);
    engine.cues = Some(sender);
    engine.test_output().unwrap();
    let (commands, receiver) = mpsc::channel();
    let shared = engine.shared.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let trace = events.clone();
    let mut generation = 0;
    output_worker(shared.clone(), receiver, initial, |shared, cues| {
        generation += 1;
        trace.lock().unwrap().push(format!("open{generation}"));
        if generation == 1 {
            shared.output_failed.store(true, Ordering::Release);
        } else {
            assert!(cues.try_recv().is_err());
            commands.send(OutputCommand::Stop).unwrap();
        }
        Ok::<_, String>(Held {
            generation,
            events: trace.clone(),
            _cues: cues,
        })
    });
    assert_eq!(
        *events.lock().unwrap(),
        ["open1", "drop1", "open2", "drop2"]
    );
    assert_eq!(engine.current_mix().master, mix.master);
    assert!(engine.current_mix().muted && engine.current_mix().ducked);
    assert_eq!(shared.tests_consumed.load(Ordering::Relaxed), 0);
    assert_eq!(shared.tests_interrupted.load(Ordering::Relaxed), 1);
    assert!(shared.last_test_interrupted.load(Ordering::Relaxed));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audio_recovery_interruption_waits_for_accepted_test_bookkeeping() {
    let engine = SoundEngine::default();
    let shared = engine.shared.clone();
    let accepted = shared.test_bookkeeping.lock().unwrap();
    let (ready, waiting) = mpsc::channel();
    let worker_shared = shared.clone();
    let worker = std::thread::spawn(move || {
        ready.send(()).unwrap();
        interrupt_output_tests(&worker_shared);
    });
    waiting.recv().unwrap();
    // Force recovery to encounter the exact acceptance/accounting window.
    // It must wait for the same metadata lock used by test_output().
    shared.tests_queued.fetch_add(1, Ordering::Relaxed);
    shared.last_test_interrupted.store(false, Ordering::Relaxed);
    drop(accepted);
    worker.join().unwrap();
    assert_eq!(shared.tests_interrupted.load(Ordering::Relaxed), 1);
    assert!(shared.last_test_interrupted.load(Ordering::Relaxed));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audio_recovery_initial_open_failure_can_retry_without_restarting_engine() {
    let engine = SoundEngine::default();
    let (commands, receiver) = mpsc::channel();
    let (_sender, initial) = mpsc::sync_channel(32);
    let mut attempts = 0;
    output_worker(engine.shared.clone(), receiver, initial, |_, _| {
        attempts += 1;
        if attempts == 1 {
            commands.send(OutputCommand::Retry).unwrap();
            Err("device missing".into())
        } else {
            commands.send(OutputCommand::Stop).unwrap();
            Ok(())
        }
    });
    assert_eq!(attempts, 2);
}
#[test]
fn audio_feedback_test_reports_unavailable_busy_and_queued_without_claiming_hearing() {
    let mut engine = SoundEngine::default();
    assert!(engine.test_output().unwrap_err().contains("not started"));
    let (tx, rx) = mpsc::sync_channel(1);
    engine.cues = Some(tx);
    engine.test_output().unwrap();
    let status = engine.output_status();
    assert_eq!(status.tests_queued, 1);
    assert_eq!(status.tests_consumed, 0);
    assert!(engine.test_output().unwrap_err().contains("busy"));
    assert!(matches!(rx.try_recv().unwrap(), Cue::TestOutput));
    drop(rx);
    assert!(engine.test_output().unwrap_err().contains("unavailable"));
    assert_eq!(engine.output_status().tests_queued, 1);
}
#[test]
#[ignore = "Explicit invocation sends test tones through actual selected output; not acoustic capture"]
fn actual_output_receives_nonzero_test_pcm() {
    let mut engine = SoundEngine::default();
    engine.set_mix(Mix::default());
    engine.start();
    engine.cue(Cue::TestOutput);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while engine.shared.nonzero_frames.load(Ordering::Relaxed) < 18000
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let peak = f32::from_bits(engine.shared.peak_bits.load(Ordering::Relaxed) as u32);
    let receipt = serde_json::json!({"scope":"Actual CPAL callback PCM before output sample conversion; no microphone or system-output capture, no claim of acoustic audibility", "status":engine.status(),"callbacks":engine.shared.callbacks.load(Ordering::Relaxed),"cues_queued":engine.shared.queued.load(Ordering::Relaxed),"cues_consumed":engine.shared.consumed.load(Ordering::Relaxed),"nonzero_frames":engine.shared.nonzero_frames.load(Ordering::Relaxed),"peak":peak});
    eprintln!("{receipt}");
    if let Some(path) = std::env::var_os("ARKLINE_AUDIO_EVIDENCE") {
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
    assert_eq!(
        engine.shared.consumed.load(Ordering::Relaxed),
        1,
        "{}",
        engine.status()
    );
    assert!(
        engine.shared.nonzero_frames.load(Ordering::Relaxed) > 18000,
        "{}",
        engine.status()
    );
    assert!(peak > 0.025 && peak < 0.08, "{}", engine.status());
}
#[test]
#[ignore = "Explicit invocation plays procedural sound on the actual default device"]
fn actual_output_callback_runs() {
    let mut engine = SoundEngine::default();
    engine.start();
    engine.set_mix(Mix {
        active: true,
        engine: 0.25,
        ..Mix::default()
    });
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(
        engine.shared.callbacks.load(Ordering::Relaxed) > 0,
        "{}",
        engine.status()
    );
}
