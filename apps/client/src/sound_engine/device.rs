//! Output ownership, device recovery and bounded real-time callback.

use super::*;
#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::HostTrait;
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc, Mutex,
};

mod synthesis;
#[cfg(test)]
pub use synthesis::OfflineRenderer;
use synthesis::Synth;

struct Shared {
    mix: Mutex<Mix>,
    status: Mutex<String>,
    callbacks: AtomicU64,
    queued: AtomicU64,
    consumed: AtomicU64,
    tests_queued: AtomicU64,
    tests_consumed: AtomicU64,
    tests_interrupted: AtomicU64,
    test_bookkeeping: Mutex<()>,
    last_test_interrupted: AtomicBool,
    output_failed: AtomicBool,
    stream_generation: AtomicU64,
    pending_cues: Mutex<Option<SyncSender<Cue>>>,
    dropped: AtomicU64,
    nonzero_frames: AtomicU64,
    peak_bits: AtomicU64,
    rms_bits: AtomicU64,
    started: crate::Instant,
    last_callback_ns: AtomicU64,
}
pub struct SoundEngine {
    shared: Arc<Shared>,
    cues: Option<SyncSender<Cue>>,
    stop: Option<mpsc::Sender<OutputCommand>>,
    #[cfg(target_arch = "wasm32")]
    stream: Option<cpal::Stream>,
}
impl Default for SoundEngine {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                mix: Mutex::new(Mix::default()),
                status: Mutex::new("Audio not started".into()),
                callbacks: AtomicU64::new(0),
                queued: AtomicU64::new(0),
                consumed: AtomicU64::new(0),
                tests_queued: AtomicU64::new(0),
                tests_consumed: AtomicU64::new(0),
                tests_interrupted: AtomicU64::new(0),
                test_bookkeeping: Mutex::new(()),
                last_test_interrupted: AtomicBool::new(false),
                output_failed: AtomicBool::new(false),
                stream_generation: AtomicU64::new(0),
                pending_cues: Mutex::new(None),
                dropped: AtomicU64::new(0),
                nonzero_frames: AtomicU64::new(0),
                peak_bits: AtomicU64::new(0),
                rms_bits: AtomicU64::new(0),
                started: crate::Instant::now(),
                last_callback_ns: AtomicU64::new(0),
            }),
            cues: None,
            stop: None,
            #[cfg(target_arch = "wasm32")]
            stream: None,
        }
    }
}
impl SoundEngine {
    #[cfg(test)]
    pub(crate) fn test_output_state(&self) -> (bool, u64, Mix) {
        (
            self.cues.is_some(),
            self.shared.callbacks.load(Ordering::Relaxed),
            *self.shared.mix.lock().unwrap(),
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
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
        std::thread::spawn(move || output_worker(shared, done, receiver, open_stream));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn start(&mut self) {
        if self.stream.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(32);
        self.cues = Some(sender);
        match open_stream(self.shared.clone(), receiver) {
            Ok(stream) => self.stream = Some(stream),
            Err(error) => {
                *self.shared.status.lock().unwrap() =
                    format!("Browser audio unavailable: {error}. Retry after clicking Settings.")
            }
        }
    }
    pub fn set_mix(&mut self, mix: Mix) {
        *self.shared.mix.lock().unwrap() = mix;
        self.refresh_cue_queue();
    }
    fn refresh_cue_queue(&mut self) {
        if let Some(queue) = self.shared.pending_cues.lock().unwrap().take() {
            self.cues = Some(queue);
        }
    }
    pub fn cue(&mut self, cue: Cue) {
        self.refresh_cue_queue();
        if let Some(queue) = &self.cues {
            if queue.try_send(cue).is_ok() {
                self.shared.queued.fetch_add(1, Ordering::Relaxed);
            } else {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub fn current_mix(&self) -> Mix {
        *self.shared.mix.lock().unwrap()
    }
    /// Queue acceptance and callback receipt are separate from speaker audibility.
    pub fn test_output(&mut self) -> Result<(), String> {
        self.refresh_cue_queue();
        let queue = self
            .cues
            .as_ref()
            .ok_or("Audio output is not started · Retry audio first")?;
        // Recovery must not account for a discarded test between queue
        // acceptance and its receipt counters. The audio callback never locks this.
        let _receipt = self.shared.test_bookkeeping.lock().unwrap();
        queue
            .try_send(Cue::TestOutput)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    "Audio queue is busy · wait and retry the test".to_owned()
                }
                mpsc::TrySendError::Disconnected(_) => {
                    "Audio output is unavailable · Retry audio first".to_owned()
                }
            })?;
        self.shared.queued.fetch_add(1, Ordering::Relaxed);
        self.shared.tests_queued.fetch_add(1, Ordering::Relaxed);
        self.shared
            .last_test_interrupted
            .store(false, Ordering::Relaxed);
        Ok(())
    }
    pub fn output_status(&self) -> OutputStatus {
        let device = self.shared.status.lock().unwrap().clone();
        OutputStatus {
            device,
            diagnostics: self.status(),
            tests_queued: self.shared.tests_queued.load(Ordering::Relaxed),
            tests_consumed: self.shared.tests_consumed.load(Ordering::Relaxed),
            tests_interrupted: self.shared.tests_interrupted.load(Ordering::Relaxed),
            last_test_interrupted: self.shared.last_test_interrupted.load(Ordering::Relaxed),
        }
    }
    pub fn status(&self) -> String {
        let mix = *self.shared.mix.lock().unwrap();
        let age = self
            .shared
            .started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        let last = self.shared.last_callback_ns.load(Ordering::Relaxed);
        format!("{} · callbacks {} ({} ms ago) · cues {}/{} (dropped {}) · preconversion lifetime peak {:.4}, current RMS {:.4} · master {:.0}%, effects {:.0}%{}{}",
            self.shared.status.lock().unwrap(), self.shared.callbacks.load(Ordering::Relaxed), age.saturating_sub(last)/1_000_000,
            self.shared.consumed.load(Ordering::Relaxed), self.shared.queued.load(Ordering::Relaxed), self.shared.dropped.load(Ordering::Relaxed),
            f32::from_bits(self.shared.peak_bits.load(Ordering::Relaxed) as u32), f32::from_bits(self.shared.rms_bits.load(Ordering::Relaxed) as u32),
            mix.master*100.,mix.effects*100.,if mix.muted { " · MUTED" } else { "" },if mix.ducked { " · voice ducking" } else { "" })
    }
    pub fn retry(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self
                .stop
                .as_ref()
                .is_some_and(|sender| sender.send(OutputCommand::Retry).is_ok())
            {
                return;
            }
            self.stop = None;
            self.cues = None;
            self.start();
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Browser contexts may require the user's gesture. Do not spin
            // automatic retries against suspended/permission-blocked WebAudio.
            let mix = *self.shared.mix.lock().unwrap();
            *self = Self::default();
            self.set_mix(mix);
            self.start();
        }
    }
}
impl Drop for SoundEngine {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(OutputCommand::Stop);
        }
    }
}
enum OutputCommand {
    #[cfg(not(target_arch = "wasm32"))]
    Retry,
    Stop,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct RecoveryBudget(u32);
#[cfg(not(target_arch = "wasm32"))]
impl RecoveryBudget {
    fn next_delay(&mut self) -> Option<std::time::Duration> {
        if self.0 >= 5 {
            return None;
        }
        let delay = std::time::Duration::from_secs(1 << self.0);
        self.0 += 1;
        Some(delay)
    }
    fn reset(&mut self) {
        self.0 = 0;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn interrupt_output_tests(shared: &Shared) {
    let _receipt = shared.test_bookkeeping.lock().unwrap();
    let lost = shared
        .tests_queued
        .load(Ordering::Relaxed)
        .saturating_sub(shared.tests_consumed.load(Ordering::Relaxed))
        .saturating_sub(shared.tests_interrupted.load(Ordering::Relaxed));
    if lost > 0 {
        shared.tests_interrupted.fetch_add(lost, Ordering::Relaxed);
        shared.last_test_interrupted.store(true, Ordering::Relaxed);
    }
}

// One worker owns stream creation AND destruction. Retry never joins a device
// thread on the UI/callback, and the next stream opens only after the old drops.
#[cfg(not(target_arch = "wasm32"))]
fn output_worker<S>(
    shared: Arc<Shared>,
    commands: Receiver<OutputCommand>,
    initial: Receiver<Cue>,
    mut open: impl FnMut(Arc<Shared>, Receiver<Cue>) -> Result<S, String>,
) {
    use std::time::{Duration, Instant};
    let mut initial = Some(initial);
    let mut budget = RecoveryBudget::default();
    loop {
        shared.output_failed.store(false, Ordering::Release);
        *shared.status.lock().unwrap() = "Opening audio output on current default…".into();
        let (sender, cues) = if let Some(cues) = initial.take() {
            (None, cues)
        } else {
            let (sender, cues) = mpsc::sync_channel(32);
            (Some(sender), cues)
        };
        let mut immediate = false;
        let error = match open(shared.clone(), cues) {
            Ok(stream) => {
                if let Some(sender) = sender {
                    *shared.pending_cues.lock().unwrap() = Some(sender);
                }
                let opened = Instant::now();
                let mut last_activity = Instant::now();
                let mut callbacks = shared.callbacks.load(Ordering::Relaxed);
                let error = loop {
                    match commands.recv_timeout(Duration::from_millis(250)) {
                        Ok(OutputCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return
                        }
                        Ok(OutputCommand::Retry) => {
                            budget.reset();
                            immediate = true;
                            break String::new();
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if shared.output_failed.load(Ordering::Acquire) {
                        break "Device reported a stream error".into();
                    }
                    let next = shared.callbacks.load(Ordering::Relaxed);
                    if next != callbacks {
                        callbacks = next;
                        last_activity = Instant::now();
                    }
                    if last_activity.elapsed() >= Duration::from_secs(8) {
                        break "No output callbacks for 8 seconds".into();
                    }
                    if opened.elapsed() >= Duration::from_secs(15) {
                        budget.reset();
                    }
                };
                drop(stream);
                error
            }
            Err(error) => error,
        };
        // Destroying the stream destroys its cue receiver. Never replay old
        // mechanism events/test tones after a device becomes available again.
        shared.pending_cues.lock().unwrap().take();
        interrupt_output_tests(&shared);
        if immediate {
            continue;
        }
        let wait = budget.next_delay();
        *shared.status.lock().unwrap() = match wait {
            Some(delay) => format!(
                "Audio unavailable: {error} · automatic retry {}/5 in {} s",
                budget.0,
                delay.as_secs()
            ),
            None => format!(
                "Audio unavailable: {error} · automatic retries exhausted · use Retry audio"
            ),
        };
        let command = match wait {
            Some(delay) => commands.recv_timeout(delay),
            None => commands
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match command {
            Ok(OutputCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Ok(OutputCommand::Retry) => budget.reset(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn open_stream(shared: Arc<Shared>, cues: Receiver<Cue>) -> Result<cpal::Stream, String> {
    #[cfg(not(target_arch = "wasm32"))]
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No default output device")?;
    #[cfg(target_arch = "wasm32")]
    let device: cpal::Device = {
        // CPAL 0.15's enumeration probes AudioContext through eval, which a
        // restrictive site CSP correctly rejects. Probe the global property
        // without executing code, then use CPAL's public Web Audio device.
        let context = js_sys::Reflect::get(
            &js_sys::global(),
            &wasm_bindgen::JsValue::from_str("AudioContext"),
        )
        .map_err(|_| "Browser AudioContext capability check failed")?;
        if !context.is_function() {
            return Err("This browser has no Web Audio output".into());
        }
        cpal::platform::WebAudioDevice.into()
    };
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, shared.clone(), cues),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, shared.clone(), cues),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, shared.clone(), cues),
        format => return Err(format!("Unsupported output format {format:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    let generation = shared.stream_generation.fetch_add(1, Ordering::Relaxed) + 1;
    *shared.status.lock().unwrap() = format!(
        "Audio output opened · {} · {} Hz · {:?} · {} channels · stream {}",
        device.name().unwrap_or_else(|_| "Default output".into()),
        config.sample_rate.0,
        supported.sample_format(),
        config.channels,
        generation
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
                        Ok(cue) => {
                            if matches!(cue, Cue::TestOutput) {
                                shared.tests_consumed.fetch_add(1, Ordering::Relaxed);
                            }
                            synth.cue(cue);
                            shared.consumed.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => break,
                    }
                }
                let mut peak = 0.0_f32;
                let mut energy = 0.0_f32;
                let mut nonzero = 0_u64;
                for frame in out.chunks_mut(channels) {
                    let stereo = synth.next_stereo();
                    peak = peak.max(stereo[0].abs()).max(stereo[1].abs());
                    energy += (stereo[0] * stereo[0] + stereo[1] * stereo[1]) * 0.5;
                    nonzero += u64::from(stereo.iter().any(|s| s.abs() > 1e-7));
                    let mono = (stereo[0] + stereo[1]) * 0.5;
                    for (index, channel) in frame.iter_mut().enumerate() {
                        *channel = T::from_sample(if channels == 1 || index > 1 {
                            mono
                        } else {
                            stereo[index]
                        });
                    }
                }
                shared
                    .peak_bits
                    .fetch_max(peak.to_bits() as u64, Ordering::Relaxed);
                let frames = (out.len() / channels).max(1);
                shared.rms_bits.store(
                    (energy / frames as f32).sqrt().to_bits() as u64,
                    Ordering::Relaxed,
                );
                shared.nonzero_frames.fetch_add(nonzero, Ordering::Relaxed);
                shared.last_callback_ns.store(
                    shared.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                    Ordering::Relaxed,
                );
                shared.callbacks.fetch_add(1, Ordering::Relaxed);
            },
            move |error| {
                errors.output_failed.store(true, Ordering::Release);
                if let Ok(mut status) = errors.status.try_lock() {
                    let recovery = if cfg!(target_arch = "wasm32") {
                        "Click Retry audio to resume browser output."
                    } else {
                        "Automatic recovery pending."
                    };
                    *status = format!("Audio output error: {error}. {recovery}");
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
