//! Optional native speech helper. Recording starts only through an explicit call.
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

const LIMIT: usize = 2000;
const OUTPUT_LIMIT: usize = 65_536;
const DEADLINE: Duration = Duration::from_secs(30);
#[derive(Debug, PartialEq, Eq)]
pub enum VoiceEvent {
    Ready,
    Partial(String),
    Final(String),
    Error(String),
}
enum Control {
    Finish,
    Cancel,
}
enum Output {
    Event(VoiceEvent),
    Done,
}
struct Task {
    commands: Sender<Control>,
    events: Receiver<Output>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Task {
    fn drop(&mut self) {
        let _ = self.commands.send(Control::Cancel);
        // Reap before app shutdown can terminate worker threads and orphan the helper.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[derive(Default)]
pub struct Voice {
    listening: Option<Task>,
    speaking: Option<Task>,
    pending: VecDeque<VoiceEvent>,
    status: String,
}
impl Voice {
    pub fn supported() -> bool {
        cfg!(target_os = "macos")
    }
    pub fn is_listening(&self) -> bool {
        self.listening.is_some()
    }
    pub fn is_speaking(&self) -> bool {
        self.speaking.is_some()
    }
    pub fn status(&self) -> &str {
        if self.status.is_empty() {
            "Voice idle"
        } else {
            &self.status
        }
    }
    pub fn start_listening(&mut self) {
        self.cancel();
        match helper_path() {
            Ok(path) => {
                self.status = "Requesting microphone and speech access…".into();
                self.listening = Some(spawn(&path, None, DEADLINE));
            }
            Err(error) => {
                self.status = error.clone();
                self.pending.push_back(VoiceEvent::Error(error));
            }
        }
    }
    pub fn finish_listening(&mut self) {
        if let Some(task) = &self.listening {
            let _ = task.commands.send(Control::Finish);
            self.status = "Finishing transcript…".into();
        }
    }
    pub fn cancel(&mut self) {
        self.listening = None;
        self.speaking = None;
        self.pending.clear();
        self.status = "Voice idle".into();
    }
    pub fn stop_speaking(&mut self) {
        self.speaking = None;
        if !self.is_listening() {
            self.status = "Voice idle".into();
        }
    }
    pub fn speak(&mut self, text: &str) {
        self.cancel();
        let text = bounded(text);
        if text.trim().is_empty() {
            return;
        }
        match helper_path() {
            Ok(path) => {
                self.status = "Speaking…".into();
                self.speaking = Some(spawn(&path, Some(text), DEADLINE));
            }
            Err(error) => {
                self.status = error.clone();
                self.pending.push_back(VoiceEvent::Error(error));
            }
        }
    }
    pub fn poll(&mut self) -> Option<VoiceEvent> {
        for (task, listening) in [(&mut self.listening, true), (&mut self.speaking, false)] {
            let mut done = false;
            if let Some(active) = task.as_ref() {
                loop {
                    match active.events.try_recv() {
                        Ok(Output::Event(event)) => {
                            if !listening && !matches!(event, VoiceEvent::Error(_)) {
                                continue;
                            }
                            self.status = match &event {
                                VoiceEvent::Ready => "Listening…".into(),
                                VoiceEvent::Partial(_) => "Listening…".into(),
                                VoiceEvent::Final(_) => "Transcript ready".into(),
                                VoiceEvent::Error(error) => error.clone(),
                            };
                            self.pending.push_back(event);
                        }
                        Ok(Output::Done) => {
                            done = true;
                            break;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            done = true;
                            break;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                    }
                }
            }
            if done {
                *task = None;
                if !listening && self.pending.is_empty() {
                    self.status = "Voice idle".into();
                }
            }
        }
        self.pending.pop_front()
    }
}
fn helper_path() -> Result<PathBuf, String> {
    if !Voice::supported() {
        return Err("Native voice is currently available only on macOS.".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("Cannot locate speech helper: {e}"))?;
    Ok(exe
        .parent()
        .ok_or("Cannot locate speech helper directory.")?
        .join("arkline-speech"))
}
fn bounded(text: &str) -> String {
    text.chars().take(LIMIT).collect()
}
fn parse(line: &[u8]) -> Result<VoiceEvent, String> {
    let value: serde_json::Value = serde_json::from_slice(line)
        .map_err(|_| "Speech helper returned invalid JSON.".to_string())?;
    let text = || {
        bounded(
            value
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        )
    };
    match value.get("event").and_then(|v| v.as_str()) {
        Some("ready") => Ok(VoiceEvent::Ready),
        Some("partial") => Ok(VoiceEvent::Partial(text())),
        Some("final") => Ok(VoiceEvent::Final(text())),
        Some("error") => Ok(VoiceEvent::Error(bounded(
            value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Speech recognition failed."),
        ))),
        _ => Err("Speech helper returned an unknown event.".into()),
    }
}
fn spawn(path: &Path, speech: Option<String>, deadline: Duration) -> Task {
    let (commands, controls) = mpsc::channel();
    let (events, outputs) = mpsc::channel();
    let path = path.to_owned();
    let worker = thread::spawn(move || {
        let send_error = |message| {
            let _ = events.send(Output::Event(VoiceEvent::Error(message)));
        };
        let mut child = match Command::new(path)
            .arg(if speech.is_some() {
                "--speak"
            } else {
                "--listen"
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                send_error(format!("Cannot start native speech helper: {error}"));
                let _ = events.send(Output::Done);
                return;
            }
        };
        let mut input = child.stdin.take();
        let mut stdout = child.stdout.take().expect("piped speech output");
        let (lines_tx, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut buffer = Vec::new();
            let mut total = 0;
            let mut chunk = [0_u8; 512];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => {
                        if !buffer.is_empty() {
                            let _ = lines_tx.send(Ok(buffer));
                        }
                        break;
                    }
                    Ok(n) => {
                        total += n;
                        if total > OUTPUT_LIMIT {
                            let _ = lines_tx
                                .send(Err("Speech helper exceeded its output limit.".into()));
                            break;
                        }
                        for byte in &chunk[..n] {
                            if *byte == b'\n' {
                                if !buffer.is_empty()
                                    && lines_tx.send(Ok(std::mem::take(&mut buffer))).is_err()
                                {
                                    return;
                                }
                            } else {
                                buffer.push(*byte);
                            }
                        }
                    }
                    Err(error) => {
                        let _ = lines_tx.send(Err(format!("Cannot read speech helper: {error}")));
                        break;
                    }
                }
            }
        });
        if let Some(text) = &speech {
            if let Some(mut stdin) = input.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
        let start = Instant::now();
        let mut finish_at = None;
        let mut final_seen = false;
        let mut failed = false;
        loop {
            match controls.try_recv() {
                Ok(Control::Cancel) | Err(mpsc::TryRecvError::Disconnected) => break,
                Ok(Control::Finish) => {
                    if finish_at.is_none() {
                        if let Some(stdin) = input.as_mut() {
                            let _ = stdin.write_all(b"stop\n");
                            let _ = stdin.flush();
                        }
                        finish_at = Some(Instant::now());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            for line in lines.try_iter() {
                match line.and_then(|line| parse(&line)) {
                    Ok(event) => {
                        final_seen |= matches!(event, VoiceEvent::Final(_));
                        failed |= matches!(event, VoiceEvent::Error(_));
                        if events.send(Output::Event(event)).is_err() {
                            failed = true;
                        }
                    }
                    Err(error) => {
                        send_error(error);
                        failed = true;
                    }
                }
            }
            if failed || final_seen {
                break;
            }
            if start.elapsed() >= deadline
                || finish_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(5))
            {
                send_error("Speech helper timed out. Try again.".into());
                break;
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Reader may still be draining the last pipe bytes after process exit.
                    let _ = reader.join();
                    for line in lines.try_iter() {
                        match line.and_then(|line| parse(&line)) {
                            Ok(event) => {
                                final_seen |= matches!(event, VoiceEvent::Final(_));
                                failed |= matches!(event, VoiceEvent::Error(_));
                                let _ = events.send(Output::Event(event));
                            }
                            Err(error) => {
                                send_error(error);
                                failed = true;
                            }
                        }
                    }
                    if !failed && (!status.success() || (speech.is_none() && !final_seen)) {
                        send_error(
                            "Speech helper exited before producing a final transcript.".into(),
                        );
                    }
                    let _ = events.send(Output::Done);
                    return;
                }
                Err(error) => {
                    send_error(format!("Cannot monitor speech helper: {error}"));
                    break;
                }
                Ok(None) => {}
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let _ = events.send(Output::Done);
    });
    Task {
        commands,
        events: outputs,
        worker: Some(worker),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_bounds_unicode_and_rejects_unknown_events() {
        let line = serde_json::json!({"event":"final","text":"界".repeat(3000)}).to_string();
        assert!(
            matches!(parse(line.as_bytes()).unwrap(),VoiceEvent::Final(text) if text.chars().count()==2000)
        );
        assert!(parse(br#"{"event":"execute"}"#).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn fake_helper_stop_final_and_deadline_do_not_block_caller() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("arkline-speech-test-{}", std::process::id()));
        std::fs::write(&path,"#!/bin/sh\nprintf '%s\\n' '{\"event\":\"ready\"}'\nread line\nprintf '%s\\n' '{\"event\":\"final\",\"text\":\"gather 20 ore\"}'\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut voice = Voice {
            listening: Some(spawn(&path, None, Duration::from_secs(2))),
            ..Default::default()
        };
        voice.finish_listening();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut final_seen = false;
        while Instant::now() < deadline && voice.is_listening() {
            final_seen |=
                matches!(voice.poll(),Some(VoiceEvent::Final(text)) if text=="gather 20 ore");
            thread::sleep(Duration::from_millis(10));
        }
        while let Some(event) = voice.poll() {
            final_seen |= matches!(event, VoiceEvent::Final(_));
        }
        assert!(final_seen);
        let mut voice = Voice {
            listening: Some(spawn(&path, None, Duration::from_millis(50))),
            ..Default::default()
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut timed_out = false;
        while Instant::now() < deadline && voice.is_listening() {
            timed_out |=
                matches!(voice.poll(),Some(VoiceEvent::Error(text)) if text.contains("timed out"));
            thread::sleep(Duration::from_millis(10));
        }
        while let Some(event) = voice.poll() {
            timed_out |= matches!(event,VoiceEvent::Error(text) if text.contains("timed out"));
        }
        assert!(timed_out);
        let mut voice = Voice {
            listening: Some(spawn(&path, None, DEADLINE)),
            ..Default::default()
        };
        let started = Instant::now();
        voice.cancel();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!voice.is_listening());
        assert!(voice.poll().is_none());
        std::fs::remove_file(path).unwrap();
    }
}
