use crate::github::{self, Entry, GitHubWorkspace};
use crate::github_auth::{AuthPhase, GithubAuth};
use eframe::egui::{self, RichText};
use std::{io::Read, path::Path};

pub struct Community {
    workspace: GitHubWorkspace,
    auth: GithubAuth,
    client_id: String,
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
        let client_id = std::env::var("ARKLINE_GITHUB_CLIENT_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| option_env!("ARKLINE_GITHUB_CLIENT_ID").map(str::to_string))
            .or_else(|| {
                read_public_preference(project, "editor-github-client-id.txt")
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            })
            .unwrap_or_default();
        Self {
            auth: GithubAuth::new(client_id.trim().into()),
            client_id,
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
        self.auth.poll();
        if self
            .workspace
            .set_token(self.auth.access_token().map(str::to_owned))
            && self.started
        {
            self.refresh();
        }
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
    fn account_panel(&mut self, ui: &mut egui::Ui, project: &Path) {
        let state = self.auth.state().clone();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("GitHub account").strong().color(crate::GREEN));
                if let Some(login) = &state.login {
                    ui.label(RichText::new(format!("@{login}")).strong());
                }
                match state.phase {
                    AuthPhase::SignedOut | AuthPhase::Error => {
                        if ui.button("Sign in with GitHub").clicked() { self.auth.start(); }
                        if ui.small_button("Forget saved sign-in").clicked() { self.auth.sign_out(); }
                    }
                    AuthPhase::SignedIn => {
                        if ui.button("Sign out").clicked() { self.auth.sign_out(); }
                    }
                    AuthPhase::Restoring | AuthPhase::Requesting | AuthPhase::AwaitingAuthorization => {
                        ui.spinner();
                        if ui.button("Cancel sign-in").clicked() { self.auth.cancel(); }
                    }
                    AuthPhase::Unconfigured => { ui.label("App setup required"); }
                }
            });
            ui.label(&state.status);
            if state.phase == AuthPhase::AwaitingAuthorization {
                ui.horizontal_wrapped(|ui| {
                    if let Some(code) = &state.user_code {
                        ui.label(RichText::new(code).monospace().strong().size(22.));
                        if ui.button("Copy code").clicked() { ui.ctx().copy_text(code.clone()); }
                    }
                    if let Some(url) = &state.verification_url {
                        ui.hyperlink_to("Open GitHub to approve", url);
                    }
                    if let Some(expires) = state.expires_at_ms {
                        let seconds = expires.saturating_sub(now_ms()).div_ceil(1000);
                        ui.label(format!("Code expires in {}:{:02}", seconds / 60, seconds % 60));
                    }
                });
            }
            if state.phase == AuthPhase::SignedIn {
                ui.small(if state.persistent { "Credential stored in the OS credential store." }
                    else { "Session only · sign in again after closing the editor." });
            }
            ui.small("GitHub is optional for local development. This connects contributor tools; game-account linking is handled separately by Arkline identity.");
            ui.add_enabled_ui(!matches!(state.phase, AuthPhase::Restoring | AuthPhase::Requesting | AuthPhase::AwaitingAuthorization | AuthPhase::SignedIn), |ui| {
                egui::CollapsingHeader::new("GitHub app setup")
                    .default_open(state.phase == AuthPhase::Unconfigured)
                    .show(ui, |ui| {
                        ui.label("Use the public Client ID of a registered OAuth app with Device Flow enabled. No client secret is needed.");
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Public Client ID");
                            ui.add(egui::TextEdit::singleline(&mut self.client_id).desired_width(250.).char_limit(128));
                            if ui.button("Apply app settings").clicked() {
                                let id = self.client_id.trim();
                                if !valid_client_id(id) {
                                    self.message = "Enter a valid public Client ID (letters, numbers, dash, underscore or dot).".into();
                                } else if let Err(error) = save_public_preference(project, "editor-github-client-id.txt", id) {
                                    self.message = error;
                                } else {
                                    self.client_id = id.into();
                                    self.auth = GithubAuth::new(self.client_id.clone());
                                    self.message = "GitHub app settings saved. Sign in when credential restoration finishes.".into();
                                }
                            }
                        });
                        ui.hyperlink_to("OAuth app setup documentation", "https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow");
                        ui.small("Requested access: profile and read-only Projects. Repository write access is not requested. App settings contain only the public Client ID; credentials stay outside the project.");
                    });
            });
        });
    }
    pub fn show(&mut self, ctx: &egui::Context, project: &Path) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Community workspace");
            ui.label("Follow the roadmap, review contributions and read the official wiki.");
            ui.add_space(8.);
            self.account_panel(ui, project);
            ui.add_space(8.);
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
                else if !state.loading { ui.label("Public repository access"); }
                if let Some(time) = state.refreshed_at_ms {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    ui.label(format!("Last refresh {} s ago", now.saturating_sub(time)/1000));
                }
                if state.stale { ui.colored_label(egui::Color32::YELLOW,"Refresh incomplete · retained data may be stale"); }
            });
            if let Some(error) = &state.error { ui.colored_label(egui::Color32::LIGHT_RED,error); }
            if !self.message.is_empty() { ui.label(&self.message); }
            for warning in &state.warnings { ui.colored_label(egui::Color32::YELLOW, warning); }
            ui.small("Read-only workspace · refresh for updates. Connecting a repository does not create a local checkout or publish changes.");
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}
fn read_preference(project: &Path) -> Option<Vec<u8>> {
    read_public_preference(project, "editor-repository.txt")
}
fn read_public_preference(project: &Path, name: &str) -> Option<Vec<u8>> {
    let path = project.join(name);
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
    save_public_preference(project, "editor-repository.txt", repository)
}
fn save_public_preference(project: &Path, name: &str, value: &str) -> Result<(), String> {
    let path = project.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Preference must be an ordinary file".into());
        }
    }
    std::fs::write(path, value).map_err(|_| "Public preference could not be saved".into())
}
