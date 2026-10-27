//! Read-only, explicitly requested history for saved local asset documents.
use eframe::egui;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetFile {
    Material,
    Engineering,
}
impl AssetFile {
    fn path(self) -> &'static str {
        match self {
            Self::Material => "Assets/Pioneer.material.json",
            Self::Engineering => "Assets/Pioneer.engineering.json",
        }
    }
}
type Selection = (PathBuf, AssetFile, Option<u64>);
#[derive(Default)]
pub struct Provenance {
    pending: Option<mpsc::Receiver<(Selection, Result<Report, String>)>>,
    selection: Option<Selection>,
    result: Option<Result<Report, String>>,
}
#[derive(Debug)]
struct Report {
    path: String,
    line: usize,
    status: String,
    commit: Option<String>,
    author: String,
    summary: String,
    github: Option<String>,
}
impl Provenance {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: &Path,
        file: AssetFile,
        component_id: Option<u64>,
    ) {
        let selection = (project.to_path_buf(), file, component_id);
        if self.selection.as_ref() != Some(&selection) {
            self.selection = Some(selection.clone());
            self.result = None;
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((selection, result)) => {
                    if self.selection.as_ref() == Some(&selection) {
                        self.result = Some(result);
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result = Some(Err("History worker stopped".into()));
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        ui.separator();
        ui.label("Saved source history");
        ui.small("Attribution is for the saved component ID line, or document first line when no component is selected; it is not a complete history of every property.");
        ui.small(
            "Explicit local Git inspection; unsaved editor changes are not inspected or committed.",
        );
        if ui
            .add_enabled(
                self.pending.is_none(),
                egui::Button::new("Inspect saved source history"),
            )
            .clicked()
        {
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.result = None;
            thread::spawn(move || {
                let result = inspect(&selection.0, selection.1, selection.2);
                let _ = tx.send((selection, result));
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if self.pending.is_some() {
            ui.label("Inspecting local history…");
        }
        if let Some(result) = &self.result {
            match result {
                Err(error) => {
                    ui.label(error);
                }
                Ok(r) => {
                    ui.monospace(format!("{}:{}", r.path, r.line));
                    ui.label(&r.status);
                    if let Some(commit) = &r.commit {
                        ui.monospace(&commit[..commit.len().min(12)]);
                        ui.label(format!("Author: {}", r.author));
                        ui.label(&r.summary);
                    }
                    if let Some(url) = &r.github {
                        ui.hyperlink_to("View commit on GitHub", url);
                    }
                }
            }
        }
    }
}

const MAX_OUTPUT: usize = 256 * 1024;
/// Each command is time/output bounded, disables external diff/textconv and never invokes a shell.
fn git(project: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let mut child = Command::new("git")
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .args(args)
        .current_dir(project)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Git is unavailable".to_string())?;
    let mut stdout = child.stdout.take().ok_or("Git output unavailable")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .by_ref()
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes);
        let _ = tx.send((result, bytes));
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Cannot inspect Git command".into());
            }
        }
        if started.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Git inspection timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    let (read, bytes) = rx
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "Git output timed out")?;
    read.map_err(|_| "Cannot read Git output")?;
    if bytes.len() > MAX_OUTPUT {
        return Err("Git history output exceeds limit".into());
    }
    Ok((
        status.success(),
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}
fn github_repo(origin: &str) -> Option<String> {
    let value = origin.trim();
    let path = value
        .strip_prefix("https://github.com/")
        .or_else(|| value.strip_prefix("git@github.com:"))
        .or_else(|| value.strip_prefix("ssh://git@github.com/"))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let pieces: Vec<_> = path.split('/').collect();
    if pieces.len() != 2
        || pieces.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        })
    {
        return None;
    }
    Some(format!("https://github.com/{}/{}", pieces[0], pieces[1]))
}
fn component_line(source: &str, id: u64) -> Result<usize, String> {
    let document: crate::engineering_document::EngineeringDocument =
        serde_json::from_str(source).map_err(|_| "Saved engineering JSON is invalid")?;
    document.validate()?;
    if document.components.iter().filter(|c| c.id == id).count() != 1 {
        return Err(
            "Selected component is not present uniquely in the saved document; save it first"
                .into(),
        );
    }
    let target = format!("\"id\": {id},");
    let matches: Vec<_> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim() == target)
        .map(|(i, _)| i + 1)
        .collect();
    if matches.len() != 1 {
        return Err("Cannot map component ID to one pretty-format saved source line".into());
    }
    Ok(matches[0])
}
fn parse_blame(source: &str) -> Result<(Option<String>, String, String), String> {
    let hash = source
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .ok_or("Git returned no blame record")?;
    if !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Git returned an invalid commit ID".into());
    }
    let field = |prefix: &str| {
        source
            .lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap_or("")
            .to_string()
    };
    Ok((
        (!hash.bytes().all(|b| b == b'0')).then(|| hash.to_string()),
        field("author "),
        field("summary "),
    ))
}
fn inspect(project: &Path, file: AssetFile, id: Option<u64>) -> Result<Report, String> {
    if !git(project, &["rev-parse", "--show-toplevel"])?.0 {
        return Err("Project is not inside a Git working tree".into());
    }
    let relative = file.path();
    for path in [
        project.to_path_buf(),
        project.join("Assets"),
        project.join(relative),
    ] {
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("Linked asset paths are not inspected".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect asset path".into()),
        }
    }
    let (tracked, _) = git(project, &["ls-files", "--error-unmatch", "--", relative])?;
    if !tracked {
        return Ok(Report {
            path: relative.into(),
            line: 1,
            status: "Not tracked by Git; no committed history".into(),
            commit: None,
            author: String::new(),
            summary: String::new(),
            github: None,
        });
    }
    let mut source = Vec::new();
    std::fs::File::open(project.join(relative))
        .map_err(|_| "Saved asset is unavailable")?
        .take(64 * 1024 + 1)
        .read_to_end(&mut source)
        .map_err(|_| "Cannot read saved asset")?;
    if source.len() > 64 * 1024 {
        return Err("Saved asset exceeds 64 KiB".into());
    }
    let source = String::from_utf8(source).map_err(|_| "Saved asset is not UTF-8")?;
    match file {
        AssetFile::Material => {
            let document: crate::asset_document::MaterialDocument =
                serde_json::from_str(&source).map_err(|_| "Saved material JSON is invalid")?;
            document.validate()?;
        }
        AssetFile::Engineering => {
            let document: crate::engineering_document::EngineeringDocument =
                serde_json::from_str(&source).map_err(|_| "Saved engineering JSON is invalid")?;
            document.validate()?;
        }
    }
    let line = if file == AssetFile::Engineering {
        match id {
            Some(id) => component_line(&source, id)?,
            None => 1,
        }
    } else {
        1
    };
    let range = format!("{line},{line}");
    let (ok, blame) = git(
        project,
        &[
            "blame",
            "--no-textconv",
            "--line-porcelain",
            "-L",
            &range,
            "--",
            relative,
        ],
    )?;
    if !ok {
        return Err("No blame history available for this saved line (file may be newly staged or lack a commit)".into());
    }
    let (commit, author, summary) = parse_blame(&blame)?;
    let (ok, status) = git(
        project,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
            "--",
            relative,
        ],
    )?;
    if !ok {
        return Err("Cannot determine working tree changes".into());
    }
    let changed = !status.trim().is_empty();
    let (_, origin) = git(project, &["remote", "get-url", "origin"])?;
    let github = github_repo(&origin)
        .zip(commit.as_ref())
        .map(|(repo, commit)| format!("{repo}/commit/{commit}"));
    Ok(Report {
        path: relative.into(),
        line,
        status: if commit.is_none() {
            "Selected saved line has uncommitted changes"
        } else if changed {
            "File has staged or working-tree changes; selected line attribution shown"
        } else {
            "Saved file matches committed history"
        }
        .into(),
        commit,
        author,
        summary,
        github,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn github_links_only_use_explicit_safe_origins() {
        for origin in [
            "https://github.com/PIndustries/arkline-engine.git",
            "git@github.com:PIndustries/arkline-engine.git",
            "ssh://git@github.com/PIndustries/arkline-engine",
        ] {
            assert_eq!(
                github_repo(origin).as_deref(),
                Some("https://github.com/PIndustries/arkline-engine")
            );
        }
        for origin in [
            "https://secret@github.com/a/b",
            "https://github.com.evil/a/b",
            "https://github.com/a/b?token=secret",
            "https://github.com/a/../b",
            "file:///tmp/a",
            "https://github.com/a/b/c",
        ] {
            assert_eq!(github_repo(origin), None);
        }
    }
    #[test]
    fn persisted_component_mapping_requires_unique_valid_pretty_line() {
        let mut d = crate::engineering_document::EngineeringDocument::default();
        let id = d.add_component().unwrap();
        let pretty = serde_json::to_string_pretty(&d).unwrap();
        let line = component_line(&pretty, id).unwrap();
        assert_eq!(pretty.lines().nth(line - 1).unwrap().trim(), "\"id\": 1,");
        assert!(component_line(&pretty, 2).is_err());
        assert!(component_line(&serde_json::to_string(&d).unwrap(), id).is_err());
        d.components.push(d.components[0].clone());
        assert!(component_line(&serde_json::to_string_pretty(&d).unwrap(), id).is_err());
    }
    #[test]
    fn disposable_git_reports_untracked_committed_and_modified() {
        let root = std::env::temp_dir().join(format!(
            "arkline-provenance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        assert!(git(&root, &["init"]).unwrap().0);
        let mut d = crate::engineering_document::EngineeringDocument::default();
        let id = d.add_component().unwrap();
        crate::engineering_document::save(&root, &d).unwrap();
        assert!(inspect(&root, AssetFile::Engineering, Some(id))
            .unwrap()
            .status
            .starts_with("Not tracked"));
        assert!(
            git(&root, &["add", "--", AssetFile::Engineering.path()])
                .unwrap()
                .0
        );
        assert!(
            git(
                &root,
                &[
                    "-c",
                    "user.name=History Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-m",
                    "Candidate component"
                ]
            )
            .unwrap()
            .0
        );
        let report = inspect(&root, AssetFile::Engineering, Some(id)).unwrap();
        assert!(report.commit.is_some());
        assert_eq!(report.author, "History Test");
        assert_eq!(report.summary, "Candidate component");
        assert!(report.status.contains("matches committed"));
        d.components[0].mass_kg = 2.;
        crate::engineering_document::save(&root, &d).unwrap();
        let report = inspect(&root, AssetFile::Engineering, Some(id)).unwrap();
        assert!(report.status.contains("working-tree changes"));
        // Changing the selected ID source line must never link it to an old commit.
        d.components[0].id = 2;
        d.next_id = 3;
        crate::engineering_document::save(&root, &d).unwrap();
        let report = inspect(&root, AssetFile::Engineering, Some(2)).unwrap();
        assert!(report.commit.is_none());
        assert!(report.status.contains("uncommitted"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
