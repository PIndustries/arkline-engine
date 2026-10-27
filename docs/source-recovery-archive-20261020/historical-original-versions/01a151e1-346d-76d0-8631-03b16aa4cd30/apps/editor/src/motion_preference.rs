//! Read-only system preference plus a process-local transform animation override.
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct TransformMotion {
    pub manual: bool,
    system: bool,
    checked: Option<Instant>,
}

impl TransformMotion {
    pub fn poll(&mut self) {
        let now = Instant::now();
        if self
            .checked
            .is_none_or(|checked| now.duration_since(checked) >= Duration::from_secs(2))
        {
            self.system = system_reduces_motion();
            self.checked = Some(now);
        }
    }

    pub fn system(&self) -> bool {
        self.system
    }

    pub fn reduced(&self, animation_time: f32) -> bool {
        self.manual || self.system || !animation_time.is_finite() || animation_time <= 0.
    }
}

#[cfg(target_os = "macos")]
fn system_reduces_motion() -> bool {
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, sel};
    let Some(thread) = AnyClass::get("NSThread") else {
        return false;
    };
    // SAFETY: this class method returns BOOL and takes no arguments.
    let main: Bool = unsafe { msg_send![thread, isMainThread] };
    if !main.as_bool() {
        return false;
    }
    let Some(class) = AnyClass::get("NSWorkspace") else {
        return false;
    };
    // SAFETY: NSWorkspace's shared singleton is valid on this main-thread call.
    let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
    let Some(workspace) = (unsafe { workspace.as_ref() }) else {
        return false;
    };
    // This public AppKit preference read requires no Accessibility permission.
    let selector = sel!(accessibilityDisplayShouldReduceMotion);
    // SAFETY: respondsToSelector: returns BOOL for a selector argument.
    let supported: Bool = unsafe { msg_send![workspace, respondsToSelector: selector] };
    if !supported.as_bool() {
        return false;
    }
    // SAFETY: the supported, read-only property returns BOOL without arguments.
    let reduced: Bool = unsafe { msg_send![workspace, accessibilityDisplayShouldReduceMotion] };
    reduced.as_bool()
}

#[cfg(not(target_os = "macos"))]
fn system_reduces_motion() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_manual_and_animation_preferences_only_reduce_motion() {
        let mut motion = TransformMotion::default();
        assert!(!motion.reduced(0.1));
        motion.manual = true;
        assert!(motion.reduced(0.1));
        motion.manual = false;
        motion.system = true;
        assert!(motion.reduced(0.1));
        motion.system = false;
        assert!(motion.reduced(0.));
        assert!(motion.reduced(f32::NAN));
    }
}
