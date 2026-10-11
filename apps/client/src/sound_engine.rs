//! Original procedural audio. Device lifetime belongs to a worker; the callback
//! only copies a mix, consumes bounded cues, and renders into CPAL's buffer.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
};

#[derive(Clone, Copy, Debug)]
pub struct Mix {
    pub master: f32,
    pub ambience: f32,
    pub effects: f32,
    pub engine: f32,
    pub mining: f32,
    pub warp: f32,
    pub docked: bool,
    pub active: bool,
    pub muted: bool,
    pub ducked: bool,
}
impl Default for Mix {
    fn default() -> Self {
        Self {
            master: 0.45,
            ambience: 0.35,
            effects: 0.65,
            engine: 0.,
            mining: 0.,
            warp: 0.,
            docked: false,
            active: false,
            muted: false,
            ducked: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Connect,
    Disconnect,
    Undock,
    Dock,
    WarpCharge,
    WarpEnter,
    WarpArrival,
    Jump,
    MiningStart,
    MiningStop,
    MiningCycle,
    DroneLaunch,
    DroneRecall,
    DroneShot,
    Lock,
    LowPower,
    CargoFull,
    Ui,
}
struct Shared {
    mix: Mutex<Mix>,
    status: Mutex<String>,
    callbacks: AtomicU64,
}
pub struct SoundEngine {
    shared: Arc<Shared>,
    cues: Option<SyncSender<Cue>>,
    stop: Option<mpsc::Sender<()>>,
}
impl Default for SoundEngine {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                mix: Mutex::new(Mix::default()),
                status: Mutex::new("Audio not started".into()),
                callbacks: AtomicU64::new(0),
            }),
            cues: None,
            stop: None,
        }
    }
}
impl SoundEngine {
    pub fn start(&mut self) {
        if self.stop.is_some() {
            return;
        }
        let (cues, receiver) = mpsc::sync_channel(32);
        let (stop, done) = mpsc::channel();
        self.cues = Some(cues);
        self.stop = Some(stop);
        *self.shared.status.lock().unwrap() = "Opening audio output…".into();
        let shared = self.shared.clone();
        thread::spawn(move || match open_stream(shared.clone(), receiver) {
            Ok(stream) => {
                let _ = done.recv();
                drop(stream);
            }
            Err(error) => {
                *shared.status.lock().unwrap() =
                    format!("Audio unavailable: {error}. Retry audio output in Settings.");
            }
        });
    }
    pub fn set_mix(&mut self, mix: Mix) {
        *self.shared.mix.lock().unwrap() = mix;
    }
    pub fn cue(&mut self, cue: Cue) {
        if let Some(queue) = &self.cues {
            let _ = queue.try_send(cue);
        }
    }
    pub fn status(&self) -> String {
        self.shared.status.lock().unwrap().clone()
    }
    pub fn retry(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mix = *self.shared.mix.lock().unwrap();
        *self = Self::default();
        self.set_mix(mix);
        self.start();
    }
}
impl Drop for SoundEngine {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
fn open_stream(shared: Arc<Shared>, cues: Receiver<Cue>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No default output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, shared.clone(), cues),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, shared.clone(), cues),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, shared.clone(), cues),
        format => return Err(format!("Unsupported output format {format:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    *shared.status.lock().unwrap() = format!(
        "Audio output active · {} · {} Hz",
        device.name().unwrap_or_else(|_| "Default output".into()),
        config.sample_rate.0
    );
    Ok(stream)
}
fn stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
    cues: Receiver<Cue>,
) -> Result<cpal::Stream, String> {
    let mut synth = Synth::new(config.sample_rate.0 as f32);
    let channels = config.channels as usize;
    let errors = shared.clone();
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                if let Ok(mix) = shared.mix.try_lock() {
                    synth.mix = *mix;
                }
                for _ in 0..32 {
                    match cues.try_recv() {
                        Ok(cue) => synth.cue(cue),
                        Err(_) => break,
                    }
                }
                for frame in out.chunks_mut(channels) {
                    let sample = synth.next();
                    for channel in frame {
                        *channel = T::from_sample(sample);
                    }
                }
                shared.callbacks.fetch_add(1, Ordering::Relaxed);
            },
            move |error| {
                if let Ok(mut status) = errors.status.try_lock() {
                    *status = format!("Audio output stopped: {error}. Retry in Settings.");
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}
#[derive(Clone, Copy, Default)]
struct Tone {
    phase: f32,
    secondary_phase: f32,
    delay: f32,
    attack: f32,
    release: f32,
    harmonic: f32,
    elapsed: f32,
    duration: f32,
    frequency: f32,
    glide: f32,
    amplitude: f32,
    noise: f32,
}
struct Synth {
    mix: Mix,
    rate: f32,
    phase: [f32; 8],
    levels: [f32; 7],
    texture: [f32; 2],
    modulation: f32,
    output: f32,
    voice_gain: f32,
    tones: [Tone; 16],
    random: u32,
}
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 1.)
    } else {
        0.
    }
}
impl Synth {
    fn new(rate: f32) -> Self {
        Self {
            mix: Mix::default(),
            rate,
            phase: [0.; 8],
            levels: [0.; 7],
            texture: [0.; 2],
            modulation: 0.0,
            output: 0.0,
            voice_gain: 1.0,
            tones: [Tone::default(); 16],
            random: 0xaced1234,
        }
    }
    fn cue(&mut self, cue: Cue) {
        // Each signature is an original, bounded set of tonal/mechanical layers.
        // No recorded samples are loaded; delayed accents share the fixed pool.
        match cue {
            Cue::WarpCharge => {
                self.layer(44., 86., 1.9, 0.22, 0.012, 0., 1.2, 0.3, 0.28);
                self.layer(132., 154., 1.6, 0.07, 0., 0.2, 0.9, 0.35, 0.1);
            }
            Cue::WarpEnter => {
                self.layer(92., -48., 1.1, 0.29, 0.02, 0., 0.018, 0.65, 0.42);
                self.layer(220., -140., 0.65, 0.1, 0.01, 0.03, 0.025, 0.5, 0.2);
            }
            Cue::WarpArrival => {
                self.layer(68., -34., 1.5, 0.28, 0.018, 0., 0.018, 1.1, 0.5);
                self.layer(210., -145., 0.9, 0.08, 0.012, 0., 0.03, 0.6, 0.15);
                self.layer(330., 0., 0.28, 0.045, 0., 0.45, 0.035, 0.2, 0.1);
            }
            Cue::Jump => {
                self.layer(48., 70., 1.0, 0.23, 0.015, 0., 0.32, 0.5, 0.35);
                self.layer(180., -100., 0.8, 0.09, 0.01, 0.25, 0.04, 0.6, 0.2);
            }
            Cue::MiningStart => {
                self.layer(82., 90., 0.75, 0.14, 0.008, 0., 0.2, 0.3, 0.45);
                self.layer(420., -90., 0.3, 0.055, 0., 0.12, 0.04, 0.2, 0.2);
            }
            Cue::MiningStop => self.layer(170., -90., 0.65, 0.13, 0.008, 0., 0.04, 0.5, 0.4),
            Cue::MiningCycle => self.layer(390., 24., 0.22, 0.07, 0., 0., 0.025, 0.17, 0.35),
            Cue::Connect => {
                self.layer(330., 0., 0.25, 0.11, 0., 0., 0.025, 0.18, 0.15);
                self.layer(495., 0., 0.35, 0.1, 0., 0.14, 0.025, 0.25, 0.15);
            }
            Cue::Disconnect => {
                self.layer(330., 0., 0.25, 0.1, 0., 0., 0.025, 0.18, 0.15);
                self.layer(220., 0., 0.35, 0.09, 0., 0.14, 0.025, 0.25, 0.15);
            }
            Cue::Undock => self.layer(52., 36., 0.95, 0.18, 0.012, 0., 0.2, 0.35, 0.6),
            Cue::Dock => {
                self.layer(105., -45., 0.65, 0.18, 0.018, 0., 0.035, 0.5, 0.6);
                self.layer(440., 0., 0.2, 0.055, 0., 0.35, 0.025, 0.15, 0.1);
            }
            Cue::DroneLaunch => self.layer(180., 110., 0.45, 0.1, 0.006, 0., 0.05, 0.25, 0.4),
            Cue::DroneRecall => self.layer(290., -110., 0.45, 0.1, 0., 0., 0.05, 0.3, 0.4),
            Cue::DroneShot => self.layer(125., -65., 0.24, 0.16, 0.02, 0., 0.018, 0.18, 0.55),
            Cue::Lock => {
                self.layer(620., 0., 0.12, 0.075, 0., 0., 0.012, 0.09, 0.15);
                self.layer(830., 0., 0.16, 0.065, 0., 0.11, 0.015, 0.12, 0.15);
            }
            Cue::LowPower => {
                self.layer(196., 0., 0.26, 0.12, 0., 0., 0.035, 0.15, 0.2);
                self.layer(165., 0., 0.3, 0.11, 0., 0.3, 0.035, 0.2, 0.2);
            }
            Cue::CargoFull => {
                self.layer(440., 0., 0.18, 0.075, 0., 0., 0.025, 0.12, 0.2);
                self.layer(330., 0., 0.28, 0.08, 0., 0.18, 0.025, 0.2, 0.2);
            }
            Cue::Ui => self.layer(720., -60., 0.06, 0.055, 0., 0., 0.01, 0.045, 0.1),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn layer(
        &mut self,
        frequency: f32,
        glide: f32,
        duration: f32,
        amplitude: f32,
        noise: f32,
        delay: f32,
        attack: f32,
        release: f32,
        harmonic: f32,
    ) {
        let Some(index) = self
            .tones
            .iter()
            .position(|t| t.elapsed >= t.duration && t.delay <= 0.)
        else {
            return;
        };
        self.tones[index] = Tone {
            frequency,
            glide,
            duration,
            amplitude,
            noise,
            delay,
            attack,
            release,
            harmonic,
            ..Tone::default()
        };
    }
    fn next(&mut self) -> f32 {
        let dt = 1. / self.rate;
        let active = if self.mix.active { 1. } else { 0. };
        let targets = [
            unit(self.mix.master)
                * if self.mix.muted {
                    0.
                } else if self.mix.ducked {
                    0.22
                } else {
                    1.
                },
            unit(self.mix.ambience) * active,
            unit(self.mix.engine) * unit(self.mix.effects) * active,
            unit(self.mix.mining) * unit(self.mix.effects) * active,
            unit(self.mix.warp) * unit(self.mix.effects) * active,
            unit(self.mix.effects),
            if self.mix.docked { 1.0 } else { 0.0 },
        ];
        for (level, target) in self.levels.iter_mut().zip(targets) {
            *level += (target - *level) * (dt * 25.).min(1.);
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let noise = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        // Two low-pass stages turn broadband random samples into a quiet
        // mechanical texture instead of sending white-noise hiss to the device.
        let texture_alpha = (std::f32::consts::TAU * 320.0 * dt).min(1.0);
        self.texture[0] += (noise - self.texture[0]) * texture_alpha;
        self.texture[1] += (self.texture[0] - self.texture[1]) * texture_alpha;
        let texture = self.texture[1];
        self.modulation = (self.modulation + 0.7 * dt).fract();
        let frequencies = [
            31.0 + self.levels[6] * 12.0,
            46.0 + self.levels[2] * 52.0,
            174.0 + self.levels[3] * 28.0,
            38.0 + self.levels[4] * 125.0,
            46.3 + self.levels[2] * 52.4,
            (174.0 + self.levels[3] * 28.0) * 2.37,
            76.7 + self.levels[4] * 250.3,
            6.0 + self.levels[3] * 1.5,
        ];
        for (phase, freq) in self.phase.iter_mut().zip(frequencies) {
            *phase = (*phase + freq * dt).fract();
        }
        let tau = std::f32::consts::TAU;
        let flutter = 0.94 + (self.modulation * tau).sin() * 0.06;
        let mut sample = self.levels[1]
            * ((self.phase[0] * tau).sin() * 0.042
                + (self.phase[0] * tau * 2.0).sin() * 0.012
                + (self.phase[0] * tau * 3.0).sin() * 0.006
                + texture * 0.006);
        sample += self.levels[2]
            * flutter
            * ((self.phase[1] * tau).sin() * 0.095
                + (self.phase[4] * tau).sin() * 0.035
                + (self.phase[1] * tau * 2.0).sin() * 0.028
                + (self.phase[1] * tau * 3.0).sin() * 0.01
                + texture * 0.012);
        let resonance = 0.91 + 0.09 * (self.phase[7] * tau).sin();
        sample += self.levels[3]
            * resonance
            * ((self.phase[2] * tau).sin() * 0.045
                + (self.phase[5] * tau).sin() * 0.021
                + (self.phase[2] * tau * 3.0).sin() * 0.008
                + texture * 0.012);
        sample += self.levels[4]
            * flutter
            * ((self.phase[3] * tau).sin() * 0.09
                + (self.phase[6] * tau).sin() * 0.027
                + texture * 0.018);
        let voice_count = self
            .tones
            .iter()
            .filter(|tone| tone.elapsed < tone.duration && tone.delay <= 0.)
            .count();
        let voice_target = 1.0 / (voice_count.max(1) as f32).sqrt();
        self.voice_gain += (voice_target - self.voice_gain) * (dt * 40.0).min(1.0);
        let voice_gain = self.voice_gain;
        for tone in &mut self.tones {
            if tone.delay > 0.0 {
                tone.delay -= dt;
                continue;
            }
            if tone.elapsed < tone.duration {
                let progress = tone.elapsed / tone.duration;
                let contour = progress * progress * (3.0 - 2.0 * progress);
                let frequency = tone.frequency + tone.glide * contour;
                tone.phase = (tone.phase + frequency * dt).fract();
                tone.secondary_phase = (tone.secondary_phase + frequency * 1.007 * dt).fract();
                let attack = (tone.elapsed / tone.attack).min(1.0);
                let release = ((tone.duration - tone.elapsed) / tone.release).min(1.0);
                let envelope = (0.5 - 0.5 * (attack * std::f32::consts::PI).cos())
                    * (0.5 - 0.5 * (release * std::f32::consts::PI).cos())
                    * (1.0 - progress * 0.65);
                sample += self.levels[5]
                    * voice_gain
                    * tone.amplitude
                    * envelope
                    * ((tone.phase * tau).sin() * (0.84 - tone.harmonic * 0.3)
                        + (tone.secondary_phase * tau).sin() * 0.16
                        + (tone.phase * tau * 2.0).sin() * tone.harmonic * 0.2
                        + (tone.phase * tau * 3.0).sin() * tone.harmonic * 0.1
                        + texture * tone.noise);
                tone.elapsed += dt;
            }
        }
        let signal = sample * self.levels[0] * 0.7;
        // Headroom before a smooth safety limiter; tame any remaining transient
        // harmonics without clipping or allocating inside the audio callback.
        let limited = 0.8 * (signal / 0.8).tanh();
        self.output += (limited - self.output) * (std::f32::consts::TAU * 2200.0 * dt).min(1.0);
        if self.output.abs() < 1e-20 {
            self.output = 0.0;
        }
        self.output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        let path =
            std::env::var("ARKLINE_AUDIO_PREVIEW_PATH").expect("Explicit output path required");
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
    #[ignore = "Explicit invocation plays procedural sound on the actual default device"]
    fn actual_output_callback_runs() {
        let mut engine = SoundEngine::default();
        engine.start();
        engine.set_mix(Mix {
            active: true,
            engine: 0.25,
            ..Mix::default()
        });
        thread::sleep(std::time::Duration::from_secs(2));
        assert!(
            engine.shared.callbacks.load(Ordering::Relaxed) > 0,
            "{}",
            engine.status()
        );
    }
}
