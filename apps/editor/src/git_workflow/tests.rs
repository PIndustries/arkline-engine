//! Real Git regressions. Every mutation is confined to a disposable test fixture.
use super::ops::{execute, Outcome, Request, Snapshot};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "arkline-git-{label}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        // create_dir deliberately fails rather than adopting an existing directory.
        fs::create_dir(&root).expect("create unique fixture");
        let repo = root.join("checkout with spaces");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        configure_identity(&repo);
        fs::write(repo.join("selected.txt"), "selected baseline\n").unwrap();
        fs::write(repo.join("other.txt"), "other baseline\n").unwrap();
        fs::write(repo.join("delete me.txt"), "delete baseline\n").unwrap();
        git(
            &repo,
            &["add", "--", "selected.txt", "other.txt", "delete me.txt"],
        );
        git(&repo, &["commit", "-m", "Fixture baseline"]);
        Self { root, repo }
    }

    fn write(&self, path: &str, contents: impl AsRef<[u8]>) {
        fs::write(self.repo.join(path), contents).unwrap();
    }

    fn inspect(&self) -> Snapshot {
        inspect(&self.repo)
    }

    fn bare_remote(&self) -> PathBuf {
        let bare = self.root.join("remote with spaces.git");
        fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "--bare", "--initial-branch=main"]);
        git(
            &self.repo,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        );
        bare
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // root is exclusively allocated above; never remove the application checkout.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn command(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .output()
        .expect("installed Git must be available for Git workflow tests")
}

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = command(repo, args);
    assert!(
        output.status.success(),
        "fixture Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn git_text(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(git(repo, args))
        .unwrap()
        .trim()
        .to_owned()
}

fn configure_identity(repo: &Path) {
    git(repo, &["config", "user.name", "Arkline disposable QA"]);
    git(
        repo,
        &["config", "user.email", "disposable-qa@example.invalid"],
    );
    git(repo, &["config", "commit.gpgsign", "false"]);
}

fn run(request: Request) -> Result<Outcome, String> {
    execute(request, &AtomicBool::new(false))
}

fn inspect(path: &Path) -> Snapshot {
    run(Request::Inspect { path: path.into() })
        .unwrap_or_else(|error| panic!("inspect fixture: {error}"))
        .snapshot
        .expect("inspection returns a snapshot")
}

fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).into()).collect()
}

fn stage(repo: &Fixture, names: &[&str]) -> Snapshot {
    run(Request::Stage {
        snapshot: repo.inspect(),
        paths: paths(names),
    })
    .unwrap_or_else(|error| panic!("stage selected fixture files: {error}"));
    repo.inspect()
}

fn commit(repo: &Fixture, names: &[&str]) {
    let snapshot = stage(repo, names);
    run(Request::Commit {
        snapshot,
        paths: paths(names),
        message: "Commit only the reviewed fixture selection".into(),
    })
    .unwrap_or_else(|error| panic!("commit selected fixture files: {error}"));
}

#[test]
fn selected_commit_excludes_unselected_modifications_and_untracked_files() {
    let fixture = Fixture::new("selected-commit");
    fixture.write("selected.txt", "reviewed change\n");
    fixture.write("other.txt", "unreviewed change\n");
    fixture.write("untracked.txt", "must remain untracked\n");
    commit(&fixture, &["selected.txt"]);

    assert_eq!(
        git(
            &fixture.repo,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-z",
                "HEAD"
            ]
        ),
        b"selected.txt\0"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:other.txt"]),
        "other baseline"
    );
    assert!(fixture.repo.join("untracked.txt").exists());
    let state = fixture.inspect();
    assert!(state
        .entries
        .iter()
        .any(|entry| entry.path == "other.txt" && entry.unstaged()));
    assert!(state
        .entries
        .iter()
        .any(|entry| entry.path == "untracked.txt"));
    assert!(!state
        .entries
        .iter()
        .any(|entry| entry.path == "selected.txt"));
}

#[test]
fn unrelated_existing_staging_blocks_commit_without_changing_the_index() {
    let fixture = Fixture::new("existing-index");
    fixture.write("other.txt", "staged before opening workspace\n");
    git(&fixture.repo, &["add", "--", "other.txt"]);
    fixture.write("selected.txt", "reviewed change\n");
    let snapshot = stage(&fixture, &["selected.txt"]);
    let before = git(&fixture.repo, &["diff", "--cached", "--binary"]);
    let head = git_text(&fixture.repo, &["rev-parse", "HEAD"]);
    let result = run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Must not include preexisting staging".into(),
    });

    assert!(
        result.is_err(),
        "unrelated staging must block the selected commit"
    );
    assert_eq!(
        git(&fixture.repo, &["diff", "--cached", "--binary"]),
        before
    );
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "reviewed change\n"
    );
}

#[test]
fn partially_staged_file_is_neither_replaced_nor_committed_implicitly() {
    let fixture = Fixture::new("partial-index");
    fixture.write("selected.txt", "reviewed staged part\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    fixture.write(
        "selected.txt",
        "reviewed staged part\nunsaved review needed\n",
    );
    let snapshot = fixture.inspect();
    let index_before = git(&fixture.repo, &["show", ":selected.txt"]);
    let head_before = git_text(&fixture.repo, &["rev-parse", "HEAD"]);

    assert!(run(Request::Stage {
        snapshot: snapshot.clone(),
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert!(run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Must not replace partial staging".into(),
    })
    .is_err());
    assert_eq!(git(&fixture.repo, &["show", ":selected.txt"]), index_before);
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "reviewed staged part\nunsaved review needed\n"
    );
}

#[test]
fn stale_snapshot_and_repeat_stage_reject_before_mutation() {
    let fixture = Fixture::new("stale");
    fixture.write("selected.txt", "first reviewed version\n");
    let snapshot = fixture.inspect();
    fixture.write("selected.txt", "changed after review\n");
    assert!(run(Request::Stage {
        snapshot,
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert!(git(&fixture.repo, &["diff", "--cached"]).is_empty());

    let snapshot = fixture.inspect();
    run(Request::Stage {
        snapshot: snapshot.clone(),
        paths: paths(&["selected.txt"]),
    })
    .unwrap();
    let index = git(&fixture.repo, &["diff", "--cached", "--binary"]);
    assert!(run(Request::Stage {
        snapshot,
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert_eq!(git(&fixture.repo, &["diff", "--cached", "--binary"]), index);
}

#[test]
fn selected_unstage_preserves_worktree_and_other_staging() {
    let fixture = Fixture::new("unstage");
    fixture.write("selected.txt", "selected contents retained\n");
    fixture.write("other.txt", "other staging retained\n");
    git(&fixture.repo, &["add", "--", "selected.txt", "other.txt"]);
    let other_index = git(&fixture.repo, &["show", ":other.txt"]);
    run(Request::Unstage {
        snapshot: fixture.inspect(),
        paths: paths(&["selected.txt"]),
    })
    .unwrap();
    assert_eq!(git(&fixture.repo, &["show", ":other.txt"]), other_index);
    assert_eq!(
        git_text(&fixture.repo, &["show", ":selected.txt"]),
        "selected baseline"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "selected contents retained\n"
    );
}

#[test]
fn canceled_mutation_has_no_effect() {
    let fixture = Fixture::new("cancel");
    fixture.write("selected.txt", "must remain unstaged\n");
    let result = execute(
        Request::Stage {
            snapshot: fixture.inspect(),
            paths: paths(&["selected.txt"]),
        },
        &AtomicBool::new(true),
    );
    assert!(result.is_err());
    assert!(git(&fixture.repo, &["diff", "--cached"]).is_empty());
}

#[test]
fn missing_installed_git_is_actionable_without_changing_parent_environment() {
    const CHILD_REPO: &str = "ARKLINE_GIT_MISSING_GIT_TEST_REPO";
    if let Some(repo) = std::env::var_os(CHILD_REPO) {
        let error = run(Request::Inspect { path: repo.into() })
            .expect_err("child with an empty PATH must not find installed Git");
        assert!(error.contains("Cannot start installed Git") && error.contains("PATH"));
        return;
    }
    let fixture = Fixture::new("missing-git");
    let empty_path = fixture.root.join("empty-bin");
    fs::create_dir(&empty_path).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let test_name = format!(
        "{module}::missing_installed_git_is_actionable_without_changing_parent_environment"
    );
    // PATH is changed only for a separate test process, never globally in the
    // parallel test harness or in the editor that may be running beside it.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--test-threads=1"])
        .env(CHILD_REPO, &fixture.repo)
        .env("PATH", empty_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated missing-Git check: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "the exact child test must execute"
    );
    assert!(git(&fixture.repo, &["status", "--porcelain"]).is_empty());
}

#[test]
fn unsafe_paths_branches_and_clone_urls_are_rejected() {
    let fixture = Fixture::new("unsafe-inputs");
    fixture.write("selected.txt", "unstaged\n");
    let snapshot = fixture.inspect();
    for name in ["../outside", "/tmp/arkline-outside", ":(glob)**"] {
        assert!(
            run(Request::Stage {
                snapshot: snapshot.clone(),
                paths: paths(&[name])
            })
            .is_err(),
            "unsafe path {name}"
        );
    }
    for name in ["--upload-pack=evil", "bad..branch", "branch with spaces"] {
        assert!(
            run(Request::CreateBranch {
                snapshot: snapshot.clone(),
                name: name.into()
            })
            .is_err(),
            "unsafe branch {name}"
        );
    }
    for (index, url) in [
        "-u malicious",
        "ext::sh -c touch /tmp/no",
        "https://user:secret@github.com/owner/repo.git",
    ]
    .iter()
    .enumerate()
    {
        let destination = fixture.root.join(format!("unsafe-clone-{index}"));
        assert!(
            run(Request::Clone {
                url: (*url).into(),
                destination: destination.clone()
            })
            .is_err(),
            "unsafe clone URL {index}"
        );
        assert!(!destination.join(".git").exists());
    }
    assert!(git(&fixture.repo, &["diff", "--cached"]).is_empty());
}

#[test]
fn clone_refuses_nonempty_destination_and_preserves_it() {
    let fixture = Fixture::new("clone-destination");
    let destination = fixture.root.join("existing project");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("precious.txt"), "preserve me").unwrap();
    assert!(run(Request::Clone {
        url: "https://github.com/PIndustries/arkline-engine.git".into(),
        destination: destination.clone(),
    })
    .is_err());
    assert_eq!(
        fs::read_to_string(destination.join("precious.txt")).unwrap(),
        "preserve me"
    );
    assert!(!destination.join(".git").exists());
}

#[test]
fn clone_and_contribution_branch_use_the_requested_checkout() {
    let fixture = Fixture::new("clone-branch");
    let bare = fixture.bare_remote();
    git(&fixture.repo, &["push", "origin", "main"]);
    let destination = fixture.root.join("cloned contribution Ω");
    run(Request::Clone {
        url: bare.to_str().unwrap().into(),
        destination: destination.clone(),
    })
    .unwrap();
    let snapshot = inspect(&destination);
    assert_eq!(snapshot.repo, destination.canonicalize().unwrap());
    assert_eq!(
        snapshot.head,
        git_text(&fixture.repo, &["rev-parse", "HEAD"])
    );
    run(Request::CreateBranch {
        snapshot,
        name: "contribution/native-git".into(),
    })
    .unwrap();
    assert_eq!(
        git_text(&destination, &["branch", "--show-current"]),
        "contribution/native-git"
    );
    run(Request::SwitchBranch {
        snapshot: inspect(&destination),
        name: "main".into(),
    })
    .unwrap();
    assert_eq!(
        git_text(&destination, &["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]),
        "main"
    );
}

#[test]
fn selected_paths_support_spaces_unicode_rename_and_deletion() {
    let fixture = Fixture::new("path-edges");
    fixture.write("space Ω file.txt", "Unicode contribution\n");
    fs::rename(
        fixture.repo.join("selected.txt"),
        fixture.repo.join("renamed 船.txt"),
    )
    .unwrap();
    fs::remove_file(fixture.repo.join("delete me.txt")).unwrap();
    let snapshot = stage(
        &fixture,
        &[
            "space Ω file.txt",
            "selected.txt",
            "renamed 船.txt",
            "delete me.txt",
        ],
    );
    // Git consolidates the staged delete/add pair into one rename status entry.
    // Review and select the refreshed entry paths instead of reusing stale paths.
    let selected = snapshot
        .entries
        .iter()
        .filter(|entry| entry.staged())
        .map(|entry| entry.path.clone())
        .collect();
    run(Request::Commit {
        snapshot,
        paths: selected,
        message: "Preserve selected Unicode rename and deletion".into(),
    })
    .unwrap();

    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:space Ω file.txt"]),
        "Unicode contribution"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:renamed 船.txt"]),
        "selected baseline"
    );
    assert!(
        !command(&fixture.repo, &["cat-file", "-e", "HEAD:selected.txt"])
            .status
            .success()
    );
    assert!(
        !command(&fixture.repo, &["cat-file", "-e", "HEAD:delete me.txt"])
            .status
            .success()
    );
    assert!(fixture.inspect().entries.is_empty());
}

#[test]
fn binary_diff_is_explicit_and_does_not_modify_file() {
    let fixture = Fixture::new("binary");
    fixture.write("image.bin", b"\0old\xff\x01");
    git(&fixture.repo, &["add", "--", "image.bin"]);
    git(&fixture.repo, &["commit", "-m", "Binary fixture"]);
    fixture.write("image.bin", b"\0new\xff\x02");
    let before = fs::read(fixture.repo.join("image.bin")).unwrap();
    let outcome = run(Request::Diff {
        snapshot: fixture.inspect(),
        paths: paths(&["image.bin"]),
        staged: false,
    })
    .unwrap();
    let diff = outcome.diff.expect("diff result");
    assert!(
        diff.to_ascii_lowercase().contains("binary"),
        "binary limitation must be visible: {diff}"
    );
    assert_eq!(fs::read(fixture.repo.join("image.bin")).unwrap(), before);
    assert!(git(&fixture.repo, &["diff", "--cached"]).is_empty());
}

#[test]
fn branch_switch_does_not_overwrite_uncommitted_work() {
    let fixture = Fixture::new("branch-safety");
    git(&fixture.repo, &["checkout", "-b", "other-branch"]);
    fixture.write("selected.txt", "different branch contents\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    git(&fixture.repo, &["commit", "-m", "Other branch"]);
    git(&fixture.repo, &["checkout", "main"]);
    fixture.write("selected.txt", "local authoring contents\n");
    let result = run(Request::SwitchBranch {
        snapshot: fixture.inspect(),
        name: "other-branch".into(),
    });
    assert!(result.is_err());
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "local authoring contents\n"
    );
}

#[test]
fn normal_push_uses_local_bare_remote_and_rejects_divergence() {
    let fixture = Fixture::new("push");
    let bare = fixture.bare_remote();
    run(Request::Push {
        snapshot: fixture.inspect(),
        remote: "origin".into(),
    })
    .unwrap();
    assert_eq!(
        git_text(&bare, &["rev-parse", "refs/heads/main"]),
        git_text(&fixture.repo, &["rev-parse", "HEAD"])
    );

    let other = fixture.root.join("second checkout");
    git(
        &fixture.root,
        &["clone", bare.to_str().unwrap(), other.to_str().unwrap()],
    );
    configure_identity(&other);
    fs::write(other.join("other.txt"), "remote writer\n").unwrap();
    git(&other, &["add", "--", "other.txt"]);
    git(&other, &["commit", "-m", "Remote writer"]);
    git(&other, &["push", "origin", "main"]);
    let remote_head = git_text(&bare, &["rev-parse", "refs/heads/main"]);
    fixture.write("selected.txt", "local divergent writer\n");
    commit(&fixture, &["selected.txt"]);
    let local_head = git_text(&fixture.repo, &["rev-parse", "HEAD"]);
    let result = run(Request::Push {
        snapshot: fixture.inspect(),
        remote: "origin".into(),
    });
    assert!(result.is_err(), "normal push must reject remote divergence");
    assert_eq!(
        git_text(&bare, &["rev-parse", "refs/heads/main"]),
        remote_head
    );
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), local_head);
}

#[test]
fn missing_commit_identity_is_actionable_and_preserves_staging() {
    let fixture = Fixture::new("identity");
    fixture.write("selected.txt", "reviewed change\n");
    git(&fixture.repo, &["config", "user.useConfigOnly", "true"]);
    git(&fixture.repo, &["config", "user.name", ""]);
    git(&fixture.repo, &["config", "user.email", ""]);
    let snapshot = stage(&fixture, &["selected.txt"]);
    let before = git(&fixture.repo, &["diff", "--cached", "--binary"]);
    let head = git_text(&fixture.repo, &["rev-parse", "HEAD"]);
    let error = run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Missing identity must stop commit".into(),
    })
    .expect_err("missing identity must fail");
    assert!(
        error.to_ascii_lowercase().contains("identity")
            || error.to_ascii_lowercase().contains("name")
            || error.to_ascii_lowercase().contains("email"),
        "identity failure must be actionable: {error}"
    );
    assert_eq!(
        git(&fixture.repo, &["diff", "--cached", "--binary"]),
        before
    );
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), head);
}

#[test]
fn unresolved_merge_conflict_is_visible_and_not_implicitly_resolved() {
    let fixture = Fixture::new("conflict");
    git(&fixture.repo, &["checkout", "-b", "conflicting-branch"]);
    fixture.write("selected.txt", "other side\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    git(&fixture.repo, &["commit", "-m", "Other side"]);
    git(&fixture.repo, &["checkout", "main"]);
    fixture.write("selected.txt", "main side\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    git(&fixture.repo, &["commit", "-m", "Main side"]);
    assert!(!command(&fixture.repo, &["merge", "conflicting-branch"])
        .status
        .success());
    let snapshot = fixture.inspect();
    assert!(snapshot
        .entries
        .iter()
        .any(|entry| entry.path == "selected.txt" && entry.conflicted()));
    let unmerged = git(&fixture.repo, &["ls-files", "--unmerged", "-z"]);
    let contents = fs::read(fixture.repo.join("selected.txt")).unwrap();
    assert!(run(Request::Stage {
        snapshot: snapshot.clone(),
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert!(run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Conflict requires manual resolution".into(),
    })
    .is_err());
    assert_eq!(
        git(&fixture.repo, &["ls-files", "--unmerged", "-z"]),
        unmerged
    );
    assert_eq!(
        fs::read(fixture.repo.join("selected.txt")).unwrap(),
        contents
    );

    // A resolved index does not mean the merge workflow itself has finished.
    fixture.write("selected.txt", "manually resolved merge\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    let resolved_snapshot = fixture.inspect();
    let resolved_index = git(&fixture.repo, &["diff", "--cached", "--binary"]);
    let head = git_text(&fixture.repo, &["rev-parse", "HEAD"]);
    assert!(run(Request::Commit {
        snapshot: resolved_snapshot,
        paths: paths(&["selected.txt"]),
        message: "Do not finish a preexisting merge implicitly".into(),
    })
    .is_err());
    assert!(fixture.repo.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        git(&fixture.repo, &["diff", "--cached", "--binary"]),
        resolved_index
    );
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), head);
}

#[test]
fn branch_switch_preserves_work_hidden_by_index_flags() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let fixture = Fixture::new("hidden-index");
        git(&fixture.repo, &["checkout", "-b", "other-branch"]);
        fixture.write("selected.txt", "different branch contents\n");
        git(&fixture.repo, &["add", "--", "selected.txt"]);
        git(&fixture.repo, &["commit", "-m", "Other branch"]);
        git(&fixture.repo, &["checkout", "main"]);
        git(&fixture.repo, &["update-index", flag, "--", "selected.txt"]);
        fixture.write("selected.txt", "local work hidden from normal status\n");
        let snapshot = fixture.inspect();
        assert!(
            snapshot.entries.is_empty(),
            "fixture must exercise a hidden index flag"
        );
        assert!(run(Request::SwitchBranch {
            snapshot,
            name: "other-branch".into()
        })
        .is_err());
        assert_eq!(
            git_text(&fixture.repo, &["branch", "--show-current"]),
            "main"
        );
        assert_eq!(
            fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
            "local work hidden from normal status\n"
        );
    }
}

#[test]
fn local_remote_cannot_be_misrepresented_as_github_pr_handoff() {
    let fixture = Fixture::new("pr-destination");
    fixture.bare_remote();
    run(Request::Push {
        snapshot: fixture.inspect(),
        remote: "origin".into(),
    })
    .unwrap();
    let result = run(Request::PrHandoff {
        snapshot: fixture.inspect(),
        remote: "origin".into(),
    });
    assert!(
        result.is_err(),
        "a local remote has no verified GitHub PR destination"
    );
}

#[cfg(unix)]
fn executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn inspection_and_commit_do_not_execute_repository_fsmonitor_or_hooks() {
    let fixture = Fixture::new("hooks");
    let marker = fixture.root.join("hook-ran");
    let monitor = fixture.root.join("fsmonitor");
    executable(
        &monitor,
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    let hooks = fixture.root.join("hooks");
    fs::create_dir(&hooks).unwrap();
    executable(
        &hooks.join("pre-commit"),
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    git(
        &fixture.repo,
        &["config", "core.fsmonitor", monitor.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    fixture.write("selected.txt", "reviewed safe commit\n");
    commit(&fixture, &["selected.txt"]);
    assert!(
        !marker.exists(),
        "repository hooks and fsmonitor must not execute"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:selected.txt"]),
        "reviewed safe commit"
    );
}

#[cfg(unix)]
#[test]
fn selected_clean_filter_is_rejected_before_external_program_runs() {
    let fixture = Fixture::new("filter");
    let marker = fixture.root.join("filter-ran");
    fixture.write(
        ".gitattributes",
        "filtered.txt filter=unsafe\nselected.txt filter=unsafe\n",
    );
    git(&fixture.repo, &["add", "--", ".gitattributes"]);
    git(
        &fixture.repo,
        &["commit", "-m", "Filter attributes fixture"],
    );
    let filter = fixture.root.join("filter");
    executable(
        &filter,
        &format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    );
    git(
        &fixture.repo,
        &["config", "filter.unsafe.clean", filter.to_str().unwrap()],
    );
    fixture.write("filtered.txt", "must not invoke clean filter\n");
    fixture.write(
        "selected.txt",
        "tracked file must not invoke clean filter\n",
    );
    let snapshot = fixture.inspect();
    assert!(
        !marker.exists(),
        "inspection of a modified tracked file must not run its filter"
    );
    assert!(run(Request::Stage {
        snapshot: snapshot.clone(),
        paths: paths(&["filtered.txt"])
    })
    .is_err());
    assert!(run(Request::Stage {
        snapshot,
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert!(!marker.exists());
    assert!(git(&fixture.repo, &["diff", "--cached"]).is_empty());
    assert_eq!(
        fs::read_to_string(fixture.repo.join("filtered.txt")).unwrap(),
        "must not invoke clean filter\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "tracked file must not invoke clean filter\n"
    );
}

#[cfg(unix)]
#[test]
fn branch_switch_refuses_target_filters_before_checkout_program_runs() {
    let fixture = Fixture::new("branch-filter");
    git(&fixture.repo, &["checkout", "-b", "filtered-target"]);
    fixture.write(".gitattributes", "selected.txt filter=unsafe\n");
    fixture.write("selected.txt", "filtered target contents\n");
    git(
        &fixture.repo,
        &["add", "--", ".gitattributes", "selected.txt"],
    );
    git(
        &fixture.repo,
        &["commit", "-m", "Target branch with filter attributes"],
    );
    git(&fixture.repo, &["checkout", "main"]);
    let marker = fixture.root.join("branch-filter-ran");
    let filter = fixture.root.join("branch-filter");
    executable(
        &filter,
        &format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    );
    git(
        &fixture.repo,
        &["config", "filter.unsafe.clean", filter.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["config", "filter.unsafe.smudge", filter.to_str().unwrap()],
    );
    assert!(run(Request::SwitchBranch {
        snapshot: fixture.inspect(),
        name: "filtered-target".into()
    })
    .is_err());
    assert!(!marker.exists());
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "selected baseline\n"
    );
}

#[cfg(unix)]
#[test]
fn in_flight_cancel_terminates_owned_transport_without_connecting() {
    use std::{
        sync::{mpsc, Arc},
        thread,
        time::{Duration, Instant},
    };
    const CHILD_REPO: &str = "ARKLINE_GIT_CANCEL_TEST_REPO";
    if let Some(repo) = std::env::var_os(CHILD_REPO) {
        let repo = PathBuf::from(repo);
        let marker = repo.parent().unwrap().join("transport-started");
        let snapshot = inspect(&repo);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = execute(
                Request::Push {
                    snapshot,
                    remote: "origin".into(),
                },
                &worker_cancel,
            );
            let _ = sender.send(result);
        });
        let start = Instant::now();
        while !marker.exists() && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(10));
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(
            marker.exists(),
            "owned transport must start before cancellation is tested"
        );
        let cancelled_at = Instant::now();
        let error = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("cancelled transport must finish within five seconds")
            .expect_err("cancelled push must not report success");
        worker.join().unwrap();
        assert!(cancelled_at.elapsed() < Duration::from_secs(5));
        assert!(
            error.to_ascii_lowercase().contains("cancel"),
            "expected cancellation result: {error}"
        );
        return;
    }

    let fixture = Fixture::new("in-flight-cancel");
    let transport = fixture.root.join("owned-ssh");
    let marker = fixture.root.join("transport-started");
    // This helper ignores every SSH argument. It never invokes SSH or a socket.
    executable(
        &transport,
        &format!(
            "#!/bin/sh\nprintf started > '{}'\n/bin/sleep 30\n",
            marker.display()
        ),
    );
    git(
        &fixture.repo,
        &["config", "core.sshCommand", transport.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &[
            "remote",
            "add",
            "origin",
            "ssh://git@github.com/arkline-fixture/never-connected.git",
        ],
    );
    let module = module_path!().split_once("::").unwrap().1;
    let test_name =
        format!("{module}::in_flight_cancel_terminates_owned_transport_without_connecting");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--test-threads=1"])
        .env(CHILD_REPO, &fixture.repo)
        .env_remove("GIT_SSH_COMMAND")
        .env_remove("GIT_SSH")
        .env_remove("GIT_SSH_VARIANT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated in-flight cancellation: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert!(marker.exists());
    assert!(git(&fixture.repo, &["status", "--porcelain"]).is_empty());
}

#[cfg(unix)]
#[test]
fn diff_does_not_execute_repository_external_diff_or_textconv() {
    let fixture = Fixture::new("diff-programs");
    fixture.write(".gitattributes", "selected.txt diff=unsafe\n");
    git(&fixture.repo, &["add", "--", ".gitattributes"]);
    git(&fixture.repo, &["commit", "-m", "Diff attributes fixture"]);
    let marker = fixture.root.join("diff-program-ran");
    let program = fixture.root.join("diff-program");
    executable(
        &program,
        &format!("#!/bin/sh\ntouch '{}'\ncat \"$1\"\n", marker.display()),
    );
    git(
        &fixture.repo,
        &["config", "diff.external", program.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["config", "diff.unsafe.command", program.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["config", "diff.unsafe.textconv", program.to_str().unwrap()],
    );
    fixture.write("selected.txt", "reviewed ordinary diff\n");
    let result = run(Request::Diff {
        snapshot: fixture.inspect(),
        paths: paths(&["selected.txt"]),
        staged: false,
    })
    .unwrap();
    assert!(
        !marker.exists(),
        "read-only diff must not execute repository programs"
    );
    assert!(result.diff.unwrap().contains("reviewed ordinary diff"));
}

#[test]
fn branch_switch_preserves_ignored_file_that_target_branch_tracks() {
    let fixture = Fixture::new("ignored-collision");
    git(&fixture.repo, &["checkout", "-b", "tracks-precious-file"]);
    fixture.write("precious.dat", "target branch version\n");
    git(&fixture.repo, &["add", "--", "precious.dat"]);
    git(
        &fixture.repo,
        &["commit", "-m", "Target branch tracks the collision path"],
    );
    git(&fixture.repo, &["checkout", "main"]);
    fixture.write(".gitignore", "precious.dat\n");
    git(&fixture.repo, &["add", "--", ".gitignore"]);
    git(
        &fixture.repo,
        &["commit", "-m", "Main branch ignores local data"],
    );
    let precious = b"\0irreplaceable local ignored fixture contents\xff\n";
    fixture.write("precious.dat", precious);

    let snapshot = fixture.inspect();
    assert!(
        snapshot.entries.is_empty(),
        "ignored local data must exercise a normally clean status"
    );
    assert!(
        run(Request::SwitchBranch {
            snapshot,
            name: "tracks-precious-file".into(),
        })
        .is_err(),
        "switch must stop before overwriting an ignored local file"
    );
    assert_eq!(
        fs::read(fixture.repo.join("precious.dat")).unwrap(),
        precious
    );
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join(".gitignore")).unwrap(),
        "precious.dat\n"
    );
}

#[test]
fn branch_selection_does_not_expand_previous_branch_shortcut() {
    let fixture = Fixture::new("literal-branch");
    git(&fixture.repo, &["checkout", "-b", "previous-contribution"]);
    git(&fixture.repo, &["checkout", "main"]);
    assert_eq!(
        git_text(&fixture.repo, &["rev-parse", "--abbrev-ref", "@{-1}"]),
        "previous-contribution"
    );
    let snapshot = fixture.inspect();
    assert!(snapshot.entries.is_empty());
    assert!(
        run(Request::SwitchBranch {
            snapshot: snapshot.clone(),
            name: "@{-1}".into(),
        })
        .is_err(),
        "branch selection must use the displayed literal branch name"
    );
    assert!(run(Request::CreateBranch {
        snapshot,
        name: "@{-1}".into(),
    })
    .is_err());
    assert_eq!(
        git_text(&fixture.repo, &["branch", "--show-current"]),
        "main"
    );
}

#[cfg(unix)]
#[test]
fn verified_pr_handoff_uses_local_upload_pack_fixture_without_publishing() {
    const CHILD_REPO: &str = "ARKLINE_GIT_PR_HANDOFF_TEST_REPO";
    if let Some(repo) = std::env::var_os(CHILD_REPO) {
        let repo = PathBuf::from(repo);
        let snapshot = inspect(&repo);
        let head = snapshot.head.clone();
        let outcome = run(Request::PrHandoff {
            snapshot,
            remote: "origin".into(),
        })
        .expect("fixture remote HEAD and contribution commit must reconcile");
        assert_eq!(
            outcome.handoff_url.as_deref(),
            Some("https://github.com/fixture/contributions/compare/main...contribution%2F%E8%88%B9?quick_pull=1")
        );
        assert!(outcome.message.contains("Create draft pull request"));
        assert!(outcome.message.contains("does not create or submit"));
        assert_eq!(outcome.snapshot.unwrap().head, head);
        assert_eq!(
            git_text(&repo, &["branch", "--show-current"]),
            "contribution/船"
        );
        return;
    }

    let fixture = Fixture::new("pr-handoff");
    let bare = fixture.bare_remote();
    git(&fixture.repo, &["push", "origin", "main"]);
    git(&fixture.repo, &["checkout", "-b", "contribution/船"]);
    fixture.write("selected.txt", "reviewed fixture contribution\n");
    git(&fixture.repo, &["add", "--", "selected.txt"]);
    git(&fixture.repo, &["commit", "-m", "Fixture contribution"]);
    git(&fixture.repo, &["push", "origin", "contribution/船"]);
    let refs_before = git(
        &bare,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let helper = fixture.root.join("local-upload-pack");
    let marker = fixture.root.join("local-upload-pack-called");
    let shell_quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    // This is a protocol fixture, not GitHub authentication or a network test.
    // The helper ignores the displayed host/path and serves only our owned bare
    // repository. No SSH executable or socket is ever used.
    executable(
        &helper,
        &format!(
            "#!/bin/sh\nprintf 'called\\n' >> {}\nexec git upload-pack {}\n",
            shell_quote(&marker),
            shell_quote(&bare)
        ),
    );
    git(
        &fixture.repo,
        &["config", "core.sshCommand", helper.to_str().unwrap()],
    );
    git(&fixture.repo, &["config", "ssh.variant", "ssh"]);
    git(
        &fixture.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "git@github.com:fixture/contributions.git",
        ],
    );
    let module = module_path!().split_once("::").unwrap().1;
    let test_name =
        format!("{module}::verified_pr_handoff_uses_local_upload_pack_fixture_without_publishing");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--test-threads=1"])
        .env(CHILD_REPO, &fixture.repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_SSH_COMMAND")
        .env_remove("GIT_SSH")
        .env_remove("GIT_SSH_VARIANT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "local protocol PR-handoff fixture: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert!(
        marker.exists(),
        "verification must use the owned upload-pack helper"
    );
    assert_eq!(
        git(
            &bare,
            &["for-each-ref", "--format=%(refname) %(objectname)"]
        ),
        refs_before
    );
}

fn assert_no_private_indexes(repo: &Path) {
    assert!(
        !fs::read_dir(repo.join(".git")).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".arkline-index-")
        }),
        "owned private index directories must be removed"
    );
}

#[test]
fn external_staging_before_index_lock_aborts_and_preserves_external_index() {
    let fixture = Fixture::new("before-lock-race");
    fixture.write("selected.txt", "reviewed selection\n");
    fixture.write("other.txt", "external staging wins\n");
    let snapshot = fixture.inspect();
    let head = snapshot.head.clone();
    let repo = fixture.repo.clone();
    super::ops::set_before_index_lock_hook(move || {
        git(&repo, &["add", "--", "other.txt"]);
    });
    assert!(run(Request::Stage {
        snapshot,
        paths: paths(&["selected.txt"])
    })
    .is_err());
    assert_eq!(
        git_text(&fixture.repo, &["show", ":other.txt"]),
        "external staging wins"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", ":selected.txt"]),
        "selected baseline"
    );
    assert_eq!(git_text(&fixture.repo, &["rev-parse", "HEAD"]), head);
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn selected_commit_holds_index_lock_and_preserves_exact_real_index_bytes() {
    let fixture = Fixture::new("under-lock-race");
    fixture.write("selected.txt", "reviewed selection\n");
    fixture.write("other.txt", "unreviewed concurrent change\n");
    let snapshot = stage(&fixture, &["selected.txt"]);
    let index = fs::read(fixture.repo.join(".git/index")).unwrap();
    let repo = fixture.repo.clone();
    super::ops::set_after_index_lock_hook(move || {
        assert!(repo.join(".git/index.lock").exists());
        assert!(
            !command(&repo, &["add", "--", "other.txt"]).status.success(),
            "ordinary Git staging must respect the owned index lock"
        );
    });
    run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Commit only the locked reviewed index".into(),
    })
    .unwrap();
    assert_eq!(fs::read(fixture.repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        git(
            &fixture.repo,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-z",
                "HEAD"
            ]
        ),
        b"selected.txt\0"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:other.txt"]),
        "other baseline"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("other.txt")).unwrap(),
        "unreviewed concurrent change\n"
    );
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn cancellation_under_index_lock_preserves_index_and_removes_owned_files() {
    use std::sync::Arc;
    let fixture = Fixture::new("locked-cancel");
    fixture.write("selected.txt", "must remain unstaged\n");
    let snapshot = fixture.inspect();
    let index = fs::read(fixture.repo.join(".git/index")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let callback_cancel = cancel.clone();
    super::ops::set_after_index_lock_hook(move || callback_cancel.store(true, Ordering::Relaxed));
    let error = execute(
        Request::Stage {
            snapshot,
            paths: paths(&["selected.txt"]),
        },
        &cancel,
    )
    .expect_err("cancellation must stop publication");
    assert!(error.to_ascii_lowercase().contains("cancel"));
    assert_eq!(fs::read(fixture.repo.join(".git/index")).unwrap(), index);
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn cancellation_does_not_unlink_a_replacement_index_lock() {
    use std::{io::Write, sync::Arc};
    let fixture = Fixture::new("replacement-lock");
    fixture.write("selected.txt", "must remain unstaged\n");
    let snapshot = fixture.inspect();
    let index = fs::read(fixture.repo.join(".git/index")).unwrap();
    let lock = fixture.repo.join(".git/index.lock");
    let callback_lock = lock.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let callback_cancel = cancel.clone();
    super::ops::set_after_index_lock_hook(move || {
        fs::remove_file(&callback_lock).unwrap();
        let mut foreign = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&callback_lock)
            .unwrap();
        foreign
            .write_all(b"replacement lock owned by another operation")
            .unwrap();
        callback_cancel.store(true, Ordering::Relaxed);
    });
    assert!(execute(
        Request::Stage {
            snapshot,
            paths: paths(&["selected.txt"])
        },
        &cancel
    )
    .is_err());
    assert_eq!(
        fs::read(&lock).unwrap(),
        b"replacement lock owned by another operation"
    );
    assert_eq!(fs::read(fixture.repo.join(".git/index")).unwrap(), index);
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn commit_cas_preserves_external_soft_reset_and_reviewed_real_index() {
    let fixture = Fixture::new("head-cas");
    fixture.write("other.txt", "second baseline commit\n");
    git(&fixture.repo, &["add", "--", "other.txt"]);
    git(&fixture.repo, &["commit", "-m", "Second fixture parent"]);
    let external_head = git_text(&fixture.repo, &["rev-parse", "HEAD~1"]);
    fixture.write("selected.txt", "reviewed selection\n");
    let snapshot = stage(&fixture, &["selected.txt"]);
    let index = fs::read(fixture.repo.join(".git/index")).unwrap();
    let repo = fixture.repo.clone();
    super::ops::set_before_commit_cas_hook(move || {
        // reset --soft updates HEAD without taking index.lock.
        git(&repo, &["reset", "--soft", "HEAD~1"]);
    });
    assert!(run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Must not replace the outside branch update".into(),
    })
    .is_err());
    assert_eq!(
        git_text(&fixture.repo, &["rev-parse", "HEAD"]),
        external_head
    );
    assert_eq!(fs::read(fixture.repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        git_text(&fixture.repo, &["show", ":selected.txt"]),
        "reviewed selection"
    );
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn selected_commit_with_split_index_preserves_real_index_and_unselected_work() {
    let fixture = Fixture::new("split-index");
    git(&fixture.repo, &["update-index", "--split-index"]);
    fixture.write("selected.txt", "split index reviewed selection\n");
    fixture.write("other.txt", "unselected split index work\n");
    stage(&fixture, &["selected.txt"]);
    // Re-enable a real split index after editor staging so this also exercises
    // materialization of a preexisting split index during commit.
    git(&fixture.repo, &["update-index", "--split-index"]);
    let snapshot = fixture.inspect();
    let index = fs::read(fixture.repo.join(".git/index")).unwrap();
    run(Request::Commit {
        snapshot,
        paths: paths(&["selected.txt"]),
        message: "Commit reviewed split index selection".into(),
    })
    .unwrap();
    assert_eq!(fs::read(fixture.repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:selected.txt"]),
        "split index reviewed selection"
    );
    assert_eq!(
        git_text(&fixture.repo, &["show", "HEAD:other.txt"]),
        "other baseline"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("other.txt")).unwrap(),
        "unselected split index work\n"
    );
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}

#[test]
fn initial_commit_uses_unborn_branch_cas_without_changing_reviewed_index() {
    let fixture = Fixture::new("unborn-commit");
    let repo = fixture.root.join("unborn checkout");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "contribution/initial"]);
    configure_identity(&repo);
    fs::write(repo.join("first.txt"), "first reviewed file\n").unwrap();
    let snapshot = inspect(&repo);
    assert!(snapshot.head.is_empty());
    run(Request::Stage {
        snapshot,
        paths: paths(&["first.txt"]),
    })
    .unwrap();
    let snapshot = inspect(&repo);
    let index = fs::read(repo.join(".git/index")).unwrap();
    run(Request::Commit {
        snapshot,
        paths: paths(&["first.txt"]),
        message: "First reviewed contribution".into(),
    })
    .unwrap();
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        git_text(&repo, &["show", "HEAD:first.txt"]),
        "first reviewed file"
    );
    assert_eq!(git_text(&repo, &["rev-list", "--count", "HEAD"]), "1");
    assert!(!repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&repo);
}

#[test]
fn inspection_uses_literal_branch_name_when_a_tag_has_the_same_name() {
    let fixture = Fixture::new("branch-tag-inspect");
    git(&fixture.repo, &["checkout", "-b", "foo"]);
    git(&fixture.repo, &["tag", "foo"]);
    let snapshot = fixture.inspect();
    assert_eq!(snapshot.branch, "foo");
    assert!(snapshot.branches.iter().any(|branch| branch == "foo"));
    assert!(!snapshot.branches.iter().any(|branch| branch == "heads/foo"));
    assert_eq!(
        snapshot.head,
        git_text(&fixture.repo, &["rev-parse", "refs/heads/foo"])
    );
}

#[test]
fn same_name_tag_does_not_redirect_selected_commit_or_normal_push() {
    let fixture = Fixture::new("branch-tag-publish");
    git(&fixture.repo, &["checkout", "-b", "foo"]);
    git(&fixture.repo, &["tag", "foo"]);
    let original = git_text(&fixture.repo, &["rev-parse", "refs/tags/foo"]);
    fixture.write(
        "selected.txt",
        "reviewed contribution on literal foo branch\n",
    );
    commit(&fixture, &["selected.txt"]);
    let snapshot = fixture.inspect();
    assert_eq!(snapshot.branch, "foo");
    assert_ne!(snapshot.head, original);
    assert_eq!(
        git_text(&fixture.repo, &["rev-parse", "refs/heads/foo"]),
        snapshot.head
    );
    assert_eq!(
        git_text(&fixture.repo, &["rev-parse", "refs/tags/foo"]),
        original
    );
    assert_eq!(
        git_text(&fixture.repo, &["rev-parse", "refs/heads/main"]),
        original
    );

    let bare = fixture.bare_remote();
    run(Request::Push {
        snapshot: fixture.inspect(),
        remote: "origin".into(),
    })
    .unwrap();
    assert_eq!(
        git_text(&bare, &["rev-parse", "refs/heads/foo"]),
        snapshot.head
    );
    assert!(
        !command(&bare, &["show-ref", "--verify", "refs/tags/foo"])
            .status
            .success(),
        "normal branch push must not publish the same-name tag"
    );
    assert!(
        !command(&bare, &["show-ref", "--verify", "refs/heads/heads/foo"])
            .status
            .success(),
        "the shortened ambiguous spelling must never become a new remote branch"
    );
}

#[cfg(unix)]
#[test]
fn stage_and_unstage_preserve_existing_group_readable_index_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("index-mode");
    fixture.write("selected.txt", "reviewed contribution\n");
    let index = fixture.repo.join(".git/index");
    fs::set_permissions(&index, fs::Permissions::from_mode(0o640)).unwrap();
    let snapshot = stage(&fixture, &["selected.txt"]);
    assert_eq!(
        fs::metadata(&index).unwrap().permissions().mode() & 0o777,
        0o640
    );
    run(Request::Unstage {
        snapshot,
        paths: paths(&["selected.txt"]),
    })
    .unwrap();
    assert_eq!(
        fs::metadata(&index).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("selected.txt")).unwrap(),
        "reviewed contribution\n"
    );
    assert!(!fixture.repo.join(".git/index.lock").exists());
    assert_no_private_indexes(&fixture.repo);
}
