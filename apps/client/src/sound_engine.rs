//! Procedural audio vocabulary and stable engine facade.

mod device;
#[cfg(test)]
pub use device::OfflineRenderer;
pub use device::SoundEngine;

/// A bounded presentation emitter, already attenuated/panned from real world poses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SpatialSound {
    #[default]
    Handling,
    Door,
    DriveMotion,
    Mining,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SpatialLoop {
    pub sound: SpatialSound,
    pub gain: f32,
    /// -1 left, +1 right. Constant-power stereo, not a surround/HRTF model.
    pub pan: f32,
}
pub const MAX_SPATIAL_LOOPS: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpatialEvent {
    Release,
    Secure,
    DoorLatch,
    MiningImpact,
}

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
    pub spatial: [SpatialLoop; MAX_SPATIAL_LOOPS],
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
            spatial: [SpatialLoop::default(); MAX_SPATIAL_LOOPS],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Spatial {
        event: SpatialEvent,
        gain: f32,
        pan: f32,
    },
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
    TestOutput,
    HatchMove,
    HatchStop,
    HatchLatch,
    HatchBlocked,
    LiftMove,
    LiftStop,
    SeatMove,
}
pub struct OutputStatus {
    pub device: String,
    pub diagnostics: String,
    pub tests_queued: u64,
    pub tests_consumed: u64,
    pub tests_interrupted: u64,
    pub last_test_interrupted: bool,
}
