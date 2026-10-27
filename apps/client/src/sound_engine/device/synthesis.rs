//! Deterministic signal generation shared by live output and offline capture.

use super::*;

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
#[derive(Clone, Copy, Default)]
struct SpatialTone {
    phase: f32,
    elapsed: f32,
    duration: f32,
    frequency: f32,
    gain: f32,
    pan: f32,
    noise: f32,
}
pub(super) struct Synth {
    pub(super) mix: Mix,
    rate: f32,
    phase: [f32; 8],
    levels: [f32; 7],
    texture: [f32; 2],
    modulation: f32,
    output: f32,
    voice_gain: f32,
    tones: [Tone; 16],
    random: u32,
    spatial_phases: [f32; MAX_SPATIAL_LOOPS],
    spatial_levels: [f32; MAX_SPATIAL_LOOPS],
    spatial_tones: [SpatialTone; 16],
}
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 1.)
    } else {
        0.
    }
}
impl Synth {
    pub(super) fn new(rate: f32) -> Self {
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
            spatial_phases: [0.; MAX_SPATIAL_LOOPS],
            spatial_levels: [0.; MAX_SPATIAL_LOOPS],
            spatial_tones: [SpatialTone::default(); 16],
        }
    }
    pub(super) fn cue(&mut self, cue: Cue) {
        // Each signature is an original, bounded set of tonal/mechanical layers.
        // No recorded samples are loaded; delayed accents share the fixed pool.
        match cue {
            Cue::Spatial { event, gain, pan } => {
                if !self.mix.active {
                    return;
                }
                let Some(tone) = self
                    .spatial_tones
                    .iter_mut()
                    .find(|t| t.elapsed >= t.duration)
                else {
                    return;
                };
                let (frequency, duration, noise) = match event {
                    SpatialEvent::Release => (175., 0.22, 0.12),
                    SpatialEvent::Secure => (110., 0.32, 0.18),
                    SpatialEvent::DoorLatch => (82., 0.3, 0.2),
                    SpatialEvent::MiningImpact => (430., 0.26, 0.28),
                };
                *tone = SpatialTone {
                    frequency,
                    duration,
                    noise,
                    gain: unit(gain),
                    pan: finite_pan(pan),
                    ..SpatialTone::default()
                };
            }
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
            // A distinct one-second mid-band diagnostic, rather than a 60 ms UI
            // tick. It uses exactly the saved master/effects/mute/voice gain path.
            Cue::TestOutput => {
                self.layer(660., 0., 0.45, 0.20, 0., 0., 0.025, 0.12, 0.12);
                self.layer(880., 0., 0.55, 0.20, 0., 0.55, 0.025, 0.15, 0.12);
            }
            Cue::HatchMove => self.layer(155., 55., 0.55, 0.12, 0.004, 0., 0.025, 0.2, 0.35),
            Cue::HatchStop => self.layer(120., -40., 0.22, 0.10, 0.005, 0., 0.02, 0.16, 0.4),
            Cue::HatchLatch => self.layer(235., -110., 0.18, 0.13, 0.008, 0., 0.015, 0.12, 0.5),
            Cue::HatchBlocked => self.layer(95., -15., 0.32, 0.10, 0.003, 0., 0.025, 0.2, 0.4),
            Cue::LiftMove => self.layer(68., 35., 0.7, 0.14, 0.004, 0., 0.06, 0.25, 0.5),
            Cue::LiftStop => self.layer(90., -38., 0.4, 0.12, 0.006, 0., 0.025, 0.25, 0.5),
            Cue::SeatMove => self.layer(210., -55., 0.55, 0.10, 0.003, 0., 0.025, 0.2, 0.3),
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
    pub(super) fn next_stereo(&mut self) -> [f32; 2] {
        let mono = self.next();
        // A paused/disconnected/stale scene cannot leave a latched mechanical voice.
        if !self.mix.active {
            self.spatial_levels.fill(0.);
            self.spatial_tones.fill(SpatialTone::default());
            return [mono; 2];
        }
        let dt = 1. / self.rate;
        let mut result = [mono; 2];
        let gain = unit(self.mix.master)
            * unit(self.mix.effects)
            * if self.mix.muted {
                0.
            } else if self.mix.ducked {
                0.22
            } else {
                1.
            };
        for (index, emitter) in self.mix.spatial.iter().enumerate() {
            let target = unit(emitter.gain);
            self.spatial_levels[index] +=
                (target - self.spatial_levels[index]) * (dt * 40.).min(1.);
            let frequency = match emitter.sound {
                SpatialSound::Handling => 74.,
                SpatialSound::Door => 43.,
                SpatialSound::DriveMotion => 123.,
                SpatialSound::Mining => 287.,
            };
            self.spatial_phases[index] = (self.spatial_phases[index] + frequency * dt).fract();
            let phase = self.spatial_phases[index] * std::f32::consts::TAU;
            let sample =
                (phase.sin() * 0.09 + (phase * 2.17).sin() * 0.025 + self.texture[1] * 0.035)
                    * self.spatial_levels[index]
                    * gain;
            let channels = pan_gains(emitter.pan);
            for i in 0..2 {
                result[i] += sample * channels[i];
            }
        }
        for tone in &mut self.spatial_tones {
            if tone.elapsed >= tone.duration {
                continue;
            }
            tone.phase = (tone.phase + tone.frequency * dt).fract();
            let progress = tone.elapsed / tone.duration;
            let envelope = (tone.elapsed / 0.008).min(1.) * (1. - progress).powi(2);
            let sample = ((tone.phase * std::f32::consts::TAU).sin() * 0.19
                + self.texture[1] * tone.noise)
                * envelope
                * tone.gain
                * gain;
            let channels = pan_gains(tone.pan);
            for i in 0..2 {
                result[i] += sample * channels[i];
            }
            tone.elapsed += dt;
        }
        result.map(|s| {
            if s.is_finite() {
                0.8 * (s / 0.8).tanh()
            } else {
                0.
            }
        })
    }
}
fn finite_pan(pan: f32) -> f32 {
    if pan.is_finite() {
        pan.clamp(-1., 1.)
    } else {
        0.
    }
}
fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (finite_pan(pan) + 1.) * std::f32::consts::FRAC_PI_4;
    [angle.cos(), angle.sin()]
}
/// The exact device synthesizer with deterministic seed, available to native
/// acceptance captures. It does not open CPAL or imply physical speaker playback.
#[cfg(test)]
pub struct OfflineRenderer {
    synth: Synth,
}
#[cfg(test)]
impl OfflineRenderer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            synth: Synth::new(sample_rate.clamp(8_000, 192_000) as f32),
        }
    }
    pub fn update(&mut self, mix: Mix, cues: &[Cue]) {
        self.synth.mix = mix;
        for &cue in cues.iter().take(32) {
            self.synth.cue(cue);
        }
    }
    pub fn render_frames(&mut self, count: usize) -> Vec<[f32; 2]> {
        (0..count).map(|_| self.synth.next_stereo()).collect()
    }
}
#[cfg(test)]
mod tests;
