use crate::github::{self, Entry, GitHubWorkspace};
use eframe::egui::{self, RichText};
use std::{io::Read, path::Path};

pub struct Community {
    workspace: GitHubWorkspace,
    repository: String,
    started: bool,
    tab: usize,
    selected: Option<usize>,
    filter: String,
    message: String,
}
impl Community {
    pub fn open(project: &Path) -> Self {
        let repository = read_preference(project)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|repo| github::validate_repository(&repo).ok())
            .unwrap_or_else(|| github::OFFICIAL_REPOSITORY.into());
        Self {
            workspace: GitHubWorkspace::default(),
            repository,
            started: false,
            tab: 0,
            selected: None,
            filter: String::new(),
            message: String::new(),
        }
    }
    pub fn poll(&mut self) {
        self.workspace.poll();
    }
    pub fn ensure_loaded(&mut self) {
        if !self.started {
            self.refresh();
        }
    }
    fn refresh(&mut self) {
        match self.workspace.request_refresh(&self.repository) {
            Ok(()) => {
                self.started = true;
                self.selected = None;
                self.message.clear();
            }
            Err(error) => self.message = error,
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, project: &Path) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Arkline community workspace");
            ui.label("Issues, contributions, roadmap and official wiki · the game simulation is local and paused");
            ui.horizontal_wrapped(|ui| {
                ui.label("Repository");
                ui.add(egui::TextEdit::singleline(&mut self.repository).desired_width(270.));
                if ui.button("Connect repository").clicked() {
                    match github::validate_repository(&self.repository) {
                        Ok(repo) => {
                            self.repository = repo;
                            self.refresh();
                            // This preference contains only a validated owner/name, never credentials.
                            if save_preference(project, &self.repository).is_err() {
                                self.message = "Connected; repository preference could not be saved".into();
                            }
                        }
                        Err(error) => self.message = error,
                    }
                }
                if ui.button("Official repository").clicked() {
                    self.repository = github::OFFICIAL_REPOSITORY.into(); self.refresh();
                    if let Err(error) = save_preference(project, &self.repository) { self.message = error; }
                }
                if ui.add_enabled(!self.workspace.state().loading, egui::Button::new("Refresh")).clicked() { self.refresh(); }
            });
            let state = self.workspace.state();
            ui.horizontal_wrapped(|ui| {
                if state.loading { ui.spinner(); ui.label("Fetching GitHub data in background…"); }
                if let Some(login) = &state.login { ui.label(format!("GitHub: {login}")); }
                else if !state.loading { ui.label("GitHub CLI sign-in unavailable"); }
                if let Some(time) = state.refreshed_at_ms {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    ui.label(format!("Last refresh {} s ago", now.saturating_sub(time)/1000));
                }
                if state.stale { ui.colored_label(egui::Color32::YELLOW,"Refresh incomplete · retained data may be stale"); }
            });
            if let Some(error) = &state.error { ui.colored_label(egui::Color32::LIGHT_RED,error); }
            if !self.message.is_empty() { ui.label(&self.message); }
            for warning in &state.warnings { ui.colored_label(egui::Color32::YELLOW, warning); }
            ui.small("Uses your existing GitHub CLI sign-in. Read-only integration: connecting does not clone code, create a fork, commit or publish. Refresh is explicit; this is not a live event stream.");
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("Repository on GitHub",format!("https://github.com/{}",state.repository));
                ui.hyperlink_to("Official project board",github::OFFICIAL_PROJECT_URL);
                ui.hyperlink_to("Official wiki",format!("https://github.com/{}/wiki",github::OFFICIAL_REPOSITORY));
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for (i,label) in ["Issues","Pull requests","Milestones","Project","Wiki"].iter().enumerate() {
                    if ui.selectable_value(&mut self.tab,i,*label).clicked() { self.selected=None; self.filter.clear(); }
                }
            });
            let entries: Vec<Entry> = match self.tab {
                0 => state.issues.clone(),
                1 => state.pulls.clone(),
                2 => state.milestones.clone(),
                3 => state.projects.iter().map(|p| Entry {
                    title:p.title.clone(),body:format!("{}\n\n{}",p.description,p.items.iter().map(|e| format!("{} · #{} {}\n{}",e.state,e.number,e.title,e.url)).collect::<Vec<_>>().join("\n\n")),url:p.url.clone(),..Default::default()
                }).collect(),
                _ => state.wiki.iter().map(|p| Entry { title:p.title.clone(),body:p.body.clone(),url:p.url.clone(),..Default::default() }).collect(),
            };
            ui.small(if self.tab == 4 { "Official wiki pages, regardless of selected repository. Text reader; remote media is not loaded." } else { "Recent bounded results. Open GitHub for full history; permission failures are reported above." });
            ui.horizontal(|ui| { ui.label("Filter"); ui.text_edit_singleline(&mut self.filter); ui.label(format!("{} loaded",entries.len())); });
            ui.separator();
            ui.columns(2, |columns| {
                egui::ScrollArea::vertical().id_salt("community_list").show(&mut columns[0], |ui| {
                    for (i,entry) in entries.iter().enumerate() {
                        if !entry.title.to_lowercase().contains(&self.filter.to_lowercase()) { continue; }
                        let title = if entry.number>0 {format!("#{} {}",entry.number,entry.title)} else {entry.title.clone()};
                        if ui.selectable_label(self.selected==Some(i),title).clicked() {self.selected=Some(i);}
                        if !entry.state.is_empty() {ui.small(&entry.state);}
                    }
                    if entries.is_empty() {ui.label(if state.loading {"Loading…"} else {"No entries loaded for this section."});}
                });
                egui::ScrollArea::vertical().id_salt(("community_detail",self.tab,self.selected)).show(&mut columns[1],|ui| {
                    if let Some(entry)=self.selected.and_then(|i|entries.get(i)) {
                        ui.heading(&entry.title);
                        if !entry.labels.is_empty() {ui.label(entry.labels.join(" · "));}
                        if entry.url.starts_with("https://github.com/") {ui.hyperlink_to("Open on GitHub",&entry.url);}
                        ui.separator();
                        document(ui,&entry.body);
                    } else {ui.label("Select an entry to read it here.");}
                });
            });
        });
    }
}

fn document(ui: &mut egui::Ui, text: &str) {
    // A selectable native text reader. Do not execute HTML, remote embeds, or issue content.
    let mut code = false;
    for line in text.lines() {
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        let content = line.trim_start_matches('#').trim_start();
        let rich = if code {
            RichText::new(line).monospace()
        } else if line.starts_with('#') {
            RichText::new(content).strong().size(18.)
        } else {
            RichText::new(line)
        };
        ui.add(egui::Label::new(rich).wrap().selectable(true));
    }
}

fn read_preference(project: &Path) -> Option<Vec<u8>> {
    let path = project.join("editor-repository.txt");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(257)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= 256).then_some(bytes)
}
fn save_preference(project: &Path, repository: &str) -> Result<(), String> {
    github::validate_repository(repository)?;
    let path = project.join("editor-repository.txt");
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Repository preference must be an ordinary file".into());
        }
    }
    std::fs::write(path, repository).map_err(|_| "Repository preference could not be saved".into())
}
