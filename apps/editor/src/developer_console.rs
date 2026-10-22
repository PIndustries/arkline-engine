//! A bounded session console, never a system shell or a separate world connection.
use arkline_client::{ConsoleEntry, ConsoleSeverity, ConsoleSnapshot};
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

const EDITOR_LIMIT: usize = 200;
const HISTORY_LIMIT: usize = 64;
const INPUT_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shortcut {
    Toggle,
    Close,
}

/// Consume the physical console key before the embedded viewport sees it. When
/// another editor/computer field owns text input, a backtick remains ordinary text.
pub fn shortcut(ctx: &egui::Context, open: bool, other_text_input: bool) -> Option<Shortcut> {
    if open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return Some(Shortcut::Close);
    }
    if !open && other_text_input {
        return None;
    }
    let toggle = ctx.input(|i| {
        i.events.iter().any(|event| {
            matches!(event,
        egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers }
        if (*key == egui::Key::Backtick || *physical_key == Some(egui::Key::Backtick))
            && !modifiers.ctrl && !modifiers.alt && !modifiers.command)
        })
    });
    if toggle {
        ctx.input_mut(|i| {
            i.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Backtick,
                        ..
                    } | egui::Event::Key {
                        physical_key: Some(egui::Key::Backtick),
                        ..
                    }
                ) && !matches!(event, egui::Event::Text(text) if text == "`" || text == "~")
            })
        });
        Some(Shortcut::Toggle)
    } else {
        None
    }
}

#[derive(Default)]
pub struct DeveloperConsole {
    editor_entries: VecDeque<ConsoleEntry>,
    next_editor_sequence: u64,
    input: String,
    filter: String,
    source: usize,
    severity: usize,
    follow: bool,
    focus_requested: bool,
    history: VecDeque<String>,
    history_cursor: Option<usize>,
    history_draft: String,
}

pub struct Action {
    pub command: Option<String>,
    pub close: bool,
}

impl DeveloperConsole {
    pub fn new(lines: impl IntoIterator<Item = String>) -> Self {
        let mut console = Self {
            follow: true,
            ..Self::default()
        };
        for line in lines {
            console.push_back(line);
        }
        console
    }
    pub fn push_back(&mut self, message: String) {
        self.next_editor_sequence = self.next_editor_sequence.saturating_add(1);
        self.editor_entries.push_back(ConsoleEntry {
            sequence: self.next_editor_sequence,
            observed_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            server_time_ms: None,
            source: "editor".into(),
            severity: ConsoleSeverity::Info,
            message: message
                .chars()
                .filter(|ch| !ch.is_control() || *ch == '\n')
                .take(2048)
                .collect(),
        });
        while self.editor_entries.len() > EDITOR_LIMIT {
            self.editor_entries.pop_front();
        }
    }
    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }
    pub fn release_focus(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new("developer_console_command")));
        self.focus_requested = false;
    }
    fn canonical_command(text: &str) -> Option<String> {
        if text.len() > INPUT_LIMIT {
            return None;
        }
        let command = text
            .trim()
            .strip_prefix('/')
            .unwrap_or(text.trim())
            .to_ascii_lowercase();
        matches!(
            command.as_str(),
            "help" | "status" | "snapshot" | "play" | "pause" | "step"
        )
        .then_some(command)
    }
    fn remember(&mut self, command: String) {
        if self.history.back() != Some(&command) {
            self.history.push_back(command);
        }
        while self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history_cursor = None;
        self.history_draft.clear();
    }
    fn history_move(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let next = if older {
            if self.history_cursor.is_none() {
                self.history_draft = self.input.clone();
            }
            Some(
                self.history_cursor
                    .map_or(self.history.len() - 1, |index| index.saturating_sub(1)),
            )
        } else {
            self.history_cursor
                .and_then(|index| (index + 1 < self.history.len()).then_some(index + 1))
        };
        self.history_cursor = next;
        self.input = next.map_or_else(
            || self.history_draft.clone(),
            |index| self.history[index].clone(),
        );
    }
    fn matches(&self, entry: &ConsoleEntry) -> bool {
        let source = entry.source.to_ascii_lowercase();
        let source_ok = match self.source {
            1 => source.contains("server") || source.contains("authority"),
            2 => source.contains("client"),
            3 => source == "editor",
            _ => true,
        };
        let severity_ok = match self.severity {
            1 => matches!(
                entry.severity,
                ConsoleSeverity::Warning | ConsoleSeverity::Error
            ),
            2 => matches!(entry.severity, ConsoleSeverity::Error),
            _ => true,
        };
        let query = self.filter.to_ascii_lowercase();
        source_ok
            && severity_ok
            && (query.is_empty()
                || source.contains(&query)
                || entry.message.to_ascii_lowercase().contains(&query))
    }
    pub fn show(&mut self, ui: &mut egui::Ui, snapshot: &ConsoleSnapshot) -> Action {
        let mut action = Action {
            command: None,
            close: false,
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong("SESSION CONSOLE");
            ui.label(format!("{} · {}", snapshot.session_label, snapshot.pilot));
            ui.colored_label(
                match snapshot.connection_state.to_ascii_lowercase().as_str() {
                    state if state.contains("disconnected") || state.contains("error") => {
                        Color32::from_rgb(230, 126, 115)
                    }
                    state if state.contains("stale") => Color32::from_rgb(227, 181, 92),
                    _ => Color32::from_rgb(111, 207, 162),
                },
                &snapshot.connection_state,
            );
            if let Some(age) = snapshot.last_received_age_ms {
                ui.small(format!("Last receipt {age} ms ago"));
            }
            if ui.button("Close · Esc").clicked() {
                action.close = true;
            }
        });
        ui.add(egui::Label::new(RichText::new(&snapshot.stream_scope).small()).wrap());
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter logs")
                    .desired_width(180.)
                    .char_limit(128),
            );
            egui::ComboBox::from_id_salt("developer_console_source")
                .selected_text(["All sources", "Server", "Client", "Editor"][self.source])
                .show_ui(ui, |ui| {
                    for (i, name) in ["All sources", "Server", "Client", "Editor"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.source, i, name);
                    }
                });
            egui::ComboBox::from_id_salt("developer_console_severity")
                .selected_text(["All levels", "Warning + error", "Error"][self.severity])
                .show_ui(ui, |ui| {
                    for (i, name) in ["All levels", "Warning + error", "Error"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.severity, i, name);
                    }
                });
            ui.checkbox(&mut self.follow, "Follow");
            if ui
                .button("Clear editor lines")
                .on_hover_text(
                    "Clears only this local editor buffer; server records are unaffected.",
                )
                .clicked()
            {
                self.editor_entries.clear();
            }
        });
        ui.separator();
        let mut entries: Vec<_> = snapshot
            .entries
            .iter()
            .chain(self.editor_entries.iter())
            .filter(|entry| self.matches(entry))
            .collect();
        entries.sort_by_key(|entry| (entry.observed_at_ms, entry.sequence));
        egui::ScrollArea::vertical().id_salt("developer_console_log_scroll").stick_to_bottom(self.follow)
            .max_height((ui.available_height() - 62.).max(35.)).min_scrolled_height(35.).auto_shrink([false, false]).show(ui, |ui| {
            if entries.is_empty() { ui.weak("No matching session log records. This does not imply the server is healthy."); }
            for entry in entries {
                let color = match entry.severity { ConsoleSeverity::Info => Color32::from_rgb(195, 205, 214), ConsoleSeverity::Warning => Color32::from_rgb(227, 181, 92), ConsoleSeverity::Error => Color32::from_rgb(230, 126, 115) };
                let milliseconds = entry.observed_at_ms % 1000;
                let seconds = entry.observed_at_ms / 1000;
                let stamp = format!("{:02}:{:02}:{:02}.{milliseconds:03}Z", seconds / 3600 % 24, seconds / 60 % 60, seconds % 60);
                let level = match entry.severity { ConsoleSeverity::Info => "INFO", ConsoleSeverity::Warning => "WARN", ConsoleSeverity::Error => "ERROR" };
                let sim = entry.server_time_ms.map_or_else(String::new, |time| format!(" sim={time}ms"));
                ui.add(egui::Label::new(RichText::new(format!("{stamp} {level} [{}]{sim} {}", entry.source, entry.message)).monospace().color(color)).wrap());
            }
        });
        ui.separator();
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.input)
                .id(egui::Id::new("developer_console_command"))
                .hint_text("help · status · snapshot · play · pause · step")
                .desired_width(f32::INFINITY)
                .char_limit(INPUT_LIMIT),
        );
        if self.focus_requested {
            response.request_focus();
            self.focus_requested = false;
        }
        if response.has_focus() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                self.history_move(true);
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                self.history_move(false);
            }
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let text = std::mem::take(&mut self.input);
            if !text.trim().is_empty() {
                if let Some(command) = Self::canonical_command(&text) {
                    self.remember(command.clone());
                    action.command = Some(command);
                } else {
                    self.push_back("Unsupported command. Type help for the session allowlist; arbitrary input is not retained.".into());
                }
            }
            response.request_focus();
        }
        ui.small("Backtick / ~ toggles · Esc closes · ↑/↓ history · Commands use the current session authority, never an OS shell.");
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn developer_console_command_history_is_bounded_and_rejects_arbitrary_input() {
        let mut console = DeveloperConsole::default();
        for text in [
            "token secret",
            "status; rm -rf",
            "shell",
            "pause now",
            "../../file",
        ] {
            assert!(DeveloperConsole::canonical_command(text).is_none());
        }
        assert_eq!(
            DeveloperConsole::canonical_command(" /STATUS "),
            Some("status".into())
        );
        for i in 0..300 {
            console.push_back(format!("state {i}"));
            console.remember(if i % 2 == 0 {
                "status".into()
            } else {
                "help".into()
            });
        }
        assert_eq!(console.editor_entries.len(), EDITOR_LIMIT);
        assert_eq!(console.history.len(), HISTORY_LIMIT);
        console.input = "draft".into();
        console.history_move(true);
        assert_eq!(console.input, "help");
        console.history_move(false);
        assert_eq!(console.input, "draft");
    }
    #[test]
    fn developer_console_physical_key_respects_text_ownership_and_removes_toggle_text() {
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            events: vec![
                egui::Event::Key {
                    key: egui::Key::Backtick,
                    physical_key: Some(egui::Key::Backtick),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
                egui::Event::Text("~".into()),
            ],
            ..Default::default()
        };
        let _ = ctx.run(input(), |ctx| {
            assert_eq!(shortcut(ctx, false, true), None);
            assert!(ctx.input(|i| !i.events.is_empty()));
        });
        let _ = ctx.run(input(), |ctx| {
            assert_eq!(shortcut(ctx, false, false), Some(Shortcut::Toggle));
            assert!(ctx.input(|i| i.events.is_empty()));
        });
    }
}
