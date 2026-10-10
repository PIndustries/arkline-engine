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
    WarpArrival,
    Jump,
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
    phase: [f32; 4],
    levels: [f32; 5],
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
            phase: [0.; 4],
            levels: [0.; 5],
            tones: [Tone::default(); 16],
            random: 0xaced1234,
        }
    }
    fn cue(&mut self, cue: Cue) {
        let (frequency, glide, duration, amplitude, noise) = match cue {
            Cue::Connect => (380., 420., 0.45, 0.25, 0.),
            Cue::Disconnect => (420., -250., 0.4, 0.2, 0.),
            Cue::Undock => (80., 110., 0.7, 0.3, 0.1),
            Cue::Dock => (190., -100., 0.55, 0.25, 0.05),
            Cue::WarpCharge => (65., 280., 1.3, 0.35, 0.15),
            Cue::WarpArrival => (400., -340., 0.8, 0.3, 0.2),
            Cue::Jump => (100., 700., 1.0, 0.35, 0.3),
            Cue::MiningCycle => (720., 180., 0.16, 0.22, 0.05),
            Cue::DroneLaunch => (250., 350., 0.25, 0.23, 0.1),
            Cue::DroneRecall => (500., -270., 0.3, 0.2, 0.),
            Cue::DroneShot => (160., -100., 0.12, 0.3, 0.5),
            Cue::Lock => (850., 160., 0.2, 0.2, 0.),
            Cue::LowPower => (220., -60., 0.6, 0.25, 0.),
            Cue::CargoFull => (660., -220., 0.45, 0.23, 0.),
            Cue::Ui => (1100., -180., 0.045, 0.12, 0.),
        };
        let index = self
            .tones
            .iter()
            .position(|t| t.elapsed >= t.duration)
            .unwrap_or(0);
        self.tones[index] = Tone {
            frequency,
            glide,
            duration,
            amplitude,
            noise,
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
        ];
        for (level, target) in self.levels.iter_mut().zip(targets) {
            *level += (target - *level) * (dt * 40.).min(1.);
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let noise = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        let frequencies = [
            if self.mix.docked { 43. } else { 31. },
            55. + self.levels[2] * 90.,
            230. + self.levels[3] * 50.,
            70. + self.levels[4] * 620.,
        ];
        for (phase, freq) in self.phase.iter_mut().zip(frequencies) {
            *phase = (*phase + freq * dt).fract();
        }
        let tau = std::f32::consts::TAU;
        let mut sample = self.levels[1] * ((self.phase[0] * tau).sin() * 0.075 + noise * 0.012);
        sample += self.levels[2]
            * ((self.phase[1] * tau).sin() * 0.18
                + (self.phase[1] * tau * 2.).sin() * 0.045
                + noise * 0.03);
        sample += self.levels[3] * ((self.phase[2] * tau).sin() * 0.07 + noise * 0.065);
        sample += self.levels[4] * ((self.phase[3] * tau).sin() * 0.12 + noise * 0.14);
        for tone in &mut self.tones {
            if tone.elapsed < tone.duration {
                let progress = tone.elapsed / tone.duration;
                tone.phase = (tone.phase + (tone.frequency + tone.glide * progress) * dt).fract();
                let envelope = (tone.elapsed / 0.012).min(1.) * (1. - progress).powi(2);
                sample += unit(self.mix.effects)
                    * tone.amplitude
                    * envelope
                    * ((tone.phase * tau).sin() * (1. - tone.noise) + noise * tone.noise);
                tone.elapsed += dt;
            }
        }
        let signal = sample * self.levels[0];
        signal / (1. + signal.abs())
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
