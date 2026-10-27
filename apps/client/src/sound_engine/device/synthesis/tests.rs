use super::*;
#[test]
fn spatial_engine_is_deterministic_stereo_and_silent_after_pause() {
    let mut mix = Mix {
        active: true,
        ambience: 0.,
        ..Mix::default()
    };
    mix.spatial[0] = SpatialLoop {
        sound: SpatialSound::Handling,
        gain: 0.8,
        pan: -1.,
    };
    let cues = [Cue::Spatial {
        event: SpatialEvent::Secure,
        gain: 0.8,
        pan: -1.,
    }];
    let mut a = OfflineRenderer::new(48000);
    let mut b = OfflineRenderer::new(48000);
    a.update(mix, &cues);
    b.update(mix, &cues);
    let frames = a.render_frames(4800);
    assert_eq!(frames, b.render_frames(4800));
    let energy = frames.iter().fold([0_f64; 2], |mut sum, f| {
        for i in 0..2 {
            sum[i] += f64::from(f[i]).powi(2);
        }
        sum
    });
    assert!(energy[0] > 0.01 && energy[0] > energy[1] * 100.);
    assert!(frames
        .iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= 0.8));
    mix.active = false;
    a.update(mix, &[]);
    assert!(a
        .render_frames(100)
        .iter()
        .flatten()
        .all(|v| v.abs() < 1e-7));
    // An event queued while paused is consumed, not deferred into resume.
    a.update(mix, &cues);
    mix.active = true;
    mix.spatial.fill(SpatialLoop::default());
    a.update(mix, &[]);
    assert!(a
        .render_frames(4800)
        .iter()
        .flatten()
        .all(|v| v.abs() < 1e-7));
}
#[test]
fn spatial_engine_rejects_nonfinite_controls_and_bounds_dense_polyphony() {
    let mut renderer = OfflineRenderer::new(48000);
    let mut mix = Mix {
        active: true,
        ..Mix::default()
    };
    mix.spatial.fill(SpatialLoop {
        sound: SpatialSound::Mining,
        gain: 1.,
        pan: 0.,
    });
    let cues = [Cue::Spatial {
        event: SpatialEvent::MiningImpact,
        gain: 1.,
        pan: 1.,
    }; 32];
    renderer.update(mix, &cues);
    assert!(renderer
        .render_frames(4800)
        .iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= 0.8));
    mix.master = f32::NAN;
    mix.spatial[0].gain = f32::INFINITY;
    mix.spatial[0].pan = f32::NAN;
    renderer.update(
        mix,
        &[Cue::Spatial {
            event: SpatialEvent::Release,
            gain: f32::NAN,
            pan: f32::INFINITY,
        }],
    );
    assert!(renderer
        .render_frames(4800)
        .iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= 0.8));
}

#[test]
fn offline_mix_is_finite_limited_and_mute_fades() {
    let engine = SoundEngine::default();
    assert_eq!(engine.shared.callbacks.load(Ordering::Relaxed), 0);
    let mut synth = Synth::new(48000.);
    synth.mix = Mix {
        active: true,
        engine: 1.,
        mining: 1.,
        warp: 1.,
        ..Mix::default()
    };
    for _ in 0..32 {
        synth.cue(Cue::Jump);
    }
    let mut energy = 0.;
    for _ in 0..48000 {
        let x = synth.next();
        assert!(x.is_finite() && x.abs() < 1.);
        energy += x * x;
    }
    assert!(energy > 1.);
    synth.mix.muted = true;
    for _ in 0..24000 {
        synth.next();
    }
    assert!(synth.next().abs() < 0.00001);
    synth.mix.master = f32::NAN;
    assert!(synth.next().is_finite());
}
#[test]
fn idle_and_cue_envelopes_reach_silence() {
    let mut synth = Synth::new(48000.);
    assert_eq!(synth.next(), 0.);
    synth.cue(Cue::Ui);
    let mut heard = false;
    for _ in 0..48000 {
        heard |= synth.next().abs() > 0.001;
    }
    assert!(heard);
    assert_eq!(synth.next(), 0.);
}
#[test]
fn steady_layers_have_low_high_frequency_energy_and_preserve_headroom() {
    let mut synth = Synth::new(48000.0);
    synth.mix = Mix {
        master: 1.0,
        effects: 1.0,
        ambience: 1.0,
        active: true,
        engine: 1.0,
        mining: 1.0,
        warp: 1.0,
        ..Mix::default()
    };
    for _ in 0..48000 {
        synth.next();
    }
    let mut energy = 0.0;
    let mut difference_energy = 0.0;
    let mut previous = synth.next();
    let mut peak = 0.0_f32;
    for _ in 0..48000 {
        let sample = synth.next();
        energy += sample * sample;
        difference_energy += (sample - previous).powi(2);
        peak = peak.max(sample.abs());
        previous = sample;
    }
    // White-noise static has difference energy around twice signal energy;
    // this bound requires the layered mix to remain spectrally smooth.
    assert!(
        difference_energy / energy < 0.012,
        "ratio {}",
        difference_energy / energy
    );
    assert!(peak < 0.35, "peak {peak}");
}
#[test]
fn saturated_cue_pool_does_not_reset_an_audible_voice() {
    let mut synth = Synth::new(48000.0);
    for _ in 0..16 {
        synth.cue(Cue::WarpCharge);
    }
    for _ in 0..4800 {
        synth.next();
    }
    let elapsed = synth.tones[0].elapsed;
    let phase = synth.tones[0].phase;
    synth.cue(Cue::DroneShot);
    assert_eq!(synth.tones[0].elapsed, elapsed);
    assert_eq!(synth.tones[0].phase, phase);
    let mut previous = synth.next();
    let mut step = 0.0_f32;
    for _ in 0..24000 {
        let sample = synth.next();
        step = step.max((sample - previous).abs());
        previous = sample;
    }
    assert!(step < 0.015, "transient step {step}");
}
#[test]
fn complete_palette_has_finite_bounded_transients_and_silent_tails() {
    for cue in [
        Cue::Connect,
        Cue::Disconnect,
        Cue::Undock,
        Cue::Dock,
        Cue::WarpCharge,
        Cue::WarpEnter,
        Cue::WarpArrival,
        Cue::Jump,
        Cue::MiningStart,
        Cue::MiningStop,
        Cue::MiningCycle,
        Cue::DroneLaunch,
        Cue::DroneRecall,
        Cue::DroneShot,
        Cue::Lock,
        Cue::LowPower,
        Cue::CargoFull,
        Cue::Ui,
        Cue::HatchMove,
        Cue::HatchStop,
        Cue::HatchLatch,
        Cue::HatchBlocked,
        Cue::LiftMove,
        Cue::LiftStop,
        Cue::SeatMove,
    ] {
        let mut synth = Synth::new(48000.);
        synth.mix.master = 1.;
        synth.mix.effects = 1.;
        synth.cue(cue);
        let mut energy = 0.;
        let mut peak = 0.0_f32;
        for _ in 0..(48000 * 3) {
            let sample = synth.next();
            assert!(sample.is_finite());
            energy += sample * sample;
            peak = peak.max(sample.abs());
        }
        assert!(energy > 0.01, "{cue:?} is silent");
        assert!(peak < 0.5, "{cue:?} peak {peak}");
        assert!(synth.next().abs() < 1e-8, "{cue:?} never decayed");
    }
}
#[test]
#[ignore = "Explicit offline WAV export; set ARKLINE_AUDIO_PREVIEW_PATH to a new output file"]
fn export_audio_preview() {
    use std::io::Write;
    let path = std::env::var("ARKLINE_AUDIO_PREVIEW_PATH").expect("Explicit output path required");
    let mut writer = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap(),
    );
    let rate = 48000_u32;
    let count = rate * 22;
    let bytes = count * 2;
    writer.write_all(b"RIFF").unwrap();
    writer.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    writer.write_all(b"WAVEfmt ").unwrap();
    writer.write_all(&16_u32.to_le_bytes()).unwrap();
    writer.write_all(&1_u16.to_le_bytes()).unwrap();
    writer.write_all(&1_u16.to_le_bytes()).unwrap();
    writer.write_all(&rate.to_le_bytes()).unwrap();
    writer.write_all(&(rate * 2).to_le_bytes()).unwrap();
    writer.write_all(&2_u16.to_le_bytes()).unwrap();
    writer.write_all(&16_u16.to_le_bytes()).unwrap();
    writer.write_all(b"data").unwrap();
    writer.write_all(&bytes.to_le_bytes()).unwrap();
    let mut synth = Synth::new(rate as f32);
    let mut peak = 0.0_f32;
    let mut squares = 0.0_f64;
    let mut nonzero = 0_u64;
    for index in 0..count {
        let time = index as f32 / rate as f32;
        match index {
            48000 => synth.cue(Cue::Connect),
            96000 => synth.cue(Cue::Undock),
            288000 => synth.cue(Cue::MiningStart),
            408000 => synth.cue(Cue::MiningCycle),
            504000 => synth.cue(Cue::MiningStop),
            576000 => synth.cue(Cue::WarpCharge),
            672000 => synth.cue(Cue::WarpEnter),
            768000 => synth.cue(Cue::WarpArrival),
            864000 => synth.cue(Cue::Lock),
            888000 => synth.cue(Cue::LowPower),
            936000 => synth.cue(Cue::CargoFull),
            _ => {}
        }
        synth.mix.active = (2.0..4.5).contains(&time)
            || (6.0..10.5).contains(&time)
            || (12.0..16.0).contains(&time);
        synth.mix.engine = if (2.0..4.5).contains(&time) {
            ((time - 2.0) / 1.5).min(0.7)
        } else {
            0.
        };
        synth.mix.mining = if (6.0..10.5).contains(&time) {
            0.85
        } else {
            0.
        };
        synth.mix.warp = if (12.0..14.0).contains(&time) {
            (time - 12.0) / 2.0 * 0.75
        } else if (14.0..16.0).contains(&time) {
            1.0
        } else {
            0.
        };
        let sample = synth.next();
        assert!(sample.is_finite());
        peak = peak.max(sample.abs());
        squares += (sample as f64).powi(2);
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        if pcm != 0 {
            nonzero += 1;
        }
        writer.write_all(&pcm.to_le_bytes()).unwrap();
    }
    writer.flush().unwrap();
    assert!(peak > 0.01 && peak < 0.8);
    assert!(nonzero > count as u64 / 4);
    eprintln!("Preview {path}: 22.000s mono PCM16 48000Hz; peak={peak:.6}, RMS={:.6}, nonzero={nonzero}/{count}",(squares/count as f64).sqrt());
    eprintln!("Timeline: 0–1 silence;1 Connect;2–4.5 Undock+engine;4.5–6 fade/silence;6 MiningStart;6–10.5 mining loop;8.5 MiningCycle;10.5 MiningStop;11.2–12 silence;12 WarpCharge;12–14 buildup;14 WarpEnter;14–16 transit;16 WarpArrival;17.5–18 silence;18 Lock;18.5 LowPower;19.5 CargoFull;20.1–22 silence.");
}
#[test]
fn mechanical_audio_uses_saved_gains_mute_and_bounded_decay() {
    for cue in [
        Cue::HatchMove,
        Cue::HatchStop,
        Cue::HatchLatch,
        Cue::HatchBlocked,
        Cue::LiftMove,
        Cue::LiftStop,
        Cue::SeatMove,
    ] {
        for (muted, master, effects, audible) in [
            (false, 0.45, 0.65, true),
            (true, 0.45, 0.65, false),
            (false, 0.0, 0.65, false),
            (false, 0.45, 0.0, false),
        ] {
            let mut synth = Synth::new(48000.);
            synth.mix = Mix {
                muted,
                master,
                effects,
                ..Mix::default()
            };
            synth.cue(cue);
            let mut peak = 0.0_f32;
            for _ in 0..48000 {
                let sample = synth.next();
                assert!(sample.is_finite());
                peak = peak.max(sample.abs());
            }
            if audible {
                assert!(peak > 0.001 && peak < 0.2, "{cue:?}: {peak}");
            } else {
                assert_eq!(peak, 0.0, "{cue:?}");
            }
            assert!(synth.next().abs() < 1.0e-8, "{cue:?} did not decay");
        }
    }
}
#[test]
fn output_test_is_distinct_sustained_and_honors_mute_and_zero_effects() {
    for (muted, effects, expected) in [
        (false, 0.65, true),
        (true, 0.65, false),
        (false, 0.0, false),
    ] {
        let mut synth = Synth::new(48000.);
        synth.mix = Mix {
            muted,
            effects,
            ..Mix::default()
        };
        synth.cue(Cue::TestOutput);
        let mut peak = 0.0_f32;
        let mut signal_frames = 0;
        for _ in 0..57600 {
            let sample = synth.next();
            peak = peak.max(sample.abs());
            signal_frames += usize::from(sample.abs() > 0.005);
        }
        if expected {
            assert!(peak > 0.025 && peak < 0.08);
            assert!(signal_frames > 18000);
        } else {
            assert_eq!(peak, 0.);
        }
    }
}
#[test]
fn output_conversion_preserves_test_signal_for_supported_formats() {
    fn check<T>()
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
        f32: cpal::FromSample<T>,
    {
        use cpal::Sample;
        let mut synth = Synth::new(48000.);
        synth.cue(Cue::TestOutput);
        let mut peak = 0.0_f32;
        let mut energy = 0.0_f64;
        let mut nonzero = 0;
        for _ in 0..57600 {
            let original = synth.next();
            let converted = T::from_sample(original);
            let decoded = f32::from_sample(converted);
            assert!(decoded.is_finite());
            assert!((decoded - original).abs() <= 1.0 / 32768.0);
            peak = peak.max(decoded.abs());
            energy += f64::from(decoded).powi(2);
            nonzero += usize::from(decoded.abs() > 0.005);
        }
        assert!(peak > 0.025 && peak < 0.08);
        assert!(nonzero > 18000);
        assert!(energy > 0.1);
        let zero = f32::from_sample(T::from_sample(0.0));
        assert_eq!(zero, 0.0, "digital silence must preserve its midpoint");
    }
    check::<f32>();
    check::<i16>();
    check::<u16>();
}
