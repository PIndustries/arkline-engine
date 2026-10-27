//! Filesystem regressions use only disposable directories, never the application checkout.
use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-project-files-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) {
        if let Some(parent) = self.0.join(relative).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(self.0.join(relative), bytes).unwrap();
    }
    fn run(&self, request: Request) -> Result<Outcome, String> {
        execute(request, &AtomicBool::new(false))
    }
    fn open(&self, relative: &str) -> Document {
        match self
            .run(Request::Read {
                root: self.0.clone(),
                relative: relative.into(),
            })
            .unwrap()
        {
            Outcome::Opened(document) => document,
            other => panic!("expected opened document, got {other:?}"),
        }
    }
    fn save(&self, relative: &str, expected: Stamp, text: &str) -> Result<Outcome, String> {
        self.run(Request::Save {
            root: self.0.clone(),
            relative: relative.into(),
            expected,
            text: text.into(),
        })
    }
    fn list(&self, show_hidden: bool, show_ignored: bool) -> (Vec<Entry>, bool) {
        match self
            .run(Request::List {
                root: self.0.clone(),
                relative: String::new(),
                show_hidden,
                show_ignored,
            })
            .unwrap()
        {
            Outcome::Listed {
                relative,
                entries,
                truncated,
            } => {
                assert!(relative.is_empty());
                (entries, truncated)
            }
            other => panic!("expected listing, got {other:?}"),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn text_read_and_explicit_save_preserve_other_files() {
    let fixture = Fixture::new();
    fixture.write("folder with spaces/日本語.rs", "fn original() {}\n");
    fixture.write("untouched.txt", "leave this alone");
    let document = fixture.open("folder with spaces/日本語.rs");
    assert_eq!(document.kind, DocumentKind::Text);
    assert_eq!(document.text.as_deref(), Some("fn original() {}\n"));
    assert_eq!(document.bytes, 17);
    let outcome = fixture
        .save(
            &document.relative,
            document.stamp.clone(),
            "fn changed() {}\n",
        )
        .unwrap();
    let Outcome::Saved { relative, stamp } = outcome else {
        panic!("expected saved document")
    };
    assert_eq!(relative, document.relative);
    assert_ne!(stamp, document.stamp);
    assert_eq!(
        fs::read_to_string(fixture.0.join(relative)).unwrap(),
        "fn changed() {}\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("untouched.txt")).unwrap(),
        "leave this alone"
    );
    assert!(fs::read_dir(fixture.0.join("folder with spaces"))
        .unwrap()
        .all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".arkline-save-")));
}

#[test]
fn external_write_blocks_save_and_preserves_disk() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "initial\n");
    let document = fixture.open("file.rs");
    fixture.write("file.rs", "external\n");
    let error = fixture
        .save("file.rs", document.stamp, "my unsaved buffer\n")
        .unwrap_err();
    assert!(error.contains("changed on disk"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("file.rs")).unwrap(),
        "external\n"
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn same_length_external_change_and_wrong_file_stamp_are_rejected() {
    let fixture = Fixture::new();
    fixture.write("first.txt", "aaaa");
    fixture.write("second.txt", "aaaa");
    let first = fixture.open("first.txt");
    fixture.write("first.txt", "bbbb");
    assert!(fixture
        .save("first.txt", first.stamp.clone(), "cccc")
        .is_err());
    assert!(fixture.save("second.txt", first.stamp, "cccc").is_err());
    assert_eq!(fs::read(fixture.0.join("second.txt")).unwrap(), b"aaaa");
}

#[test]
fn binary_invalid_utf8_and_oversized_files_have_no_editable_contents() {
    let fixture = Fixture::new();
    fixture.write("nul.bin", b"before\0after");
    fixture.write("invalid.bin", [0xff, 0xfe, 0xfd]);
    fixture.write("large.txt", vec![b'a'; TEXT_LIMIT as usize + 1]);
    for relative in ["nul.bin", "invalid.bin", "large.txt"] {
        let document = fixture.open(relative);
        assert!(document.text.is_none());
        assert_eq!(
            document.kind,
            if relative == "large.txt" {
                DocumentKind::Oversized
            } else {
                DocumentKind::Binary
            }
        );
        assert!(fixture
            .save(relative, document.stamp, "replacement")
            .is_err());
    }
}

#[test]
fn text_at_limit_is_supported_but_over_limit_save_is_not() {
    let fixture = Fixture::new();
    fixture.write("limit.txt", vec![b'a'; TEXT_LIMIT as usize]);
    let document = fixture.open("limit.txt");
    assert_eq!(document.kind, DocumentKind::Text);
    assert!(fixture
        .save(
            "limit.txt",
            document.stamp.clone(),
            &"b".repeat(TEXT_LIMIT as usize + 1)
        )
        .is_err());
    assert!(fixture
        .save("limit.txt", document.stamp, "with\0nul")
        .is_err());
    assert_eq!(
        fs::metadata(fixture.0.join("limit.txt")).unwrap().len(),
        TEXT_LIMIT
    );
}

#[test]
fn sensitive_names_are_discoverable_but_contents_are_not_read() {
    let fixture = Fixture::new();
    for path in [
        ".env",
        ".env.local",
        "private.pem",
        "session.token",
        "id_ed25519",
        ".aws/credentials",
        ".git/config",
        ".arkline-studio/session.json",
    ] {
        fixture.write(path, "secret sentinel must not become document text");
        let document = fixture.open(path);
        assert_eq!(document.kind, DocumentKind::Restricted, "{path}");
        assert!(document.text.is_none());
        assert!(fixture.save(path, document.stamp, "replacement").is_err());
    }
    let (entries, _) = fixture.list(true, true);
    assert!(entries.iter().any(|e| e.name == ".env" && e.restricted));
    assert!(entries
        .iter()
        .any(|e| e.name == ".aws" && e.restricted && e.is_dir));
    assert!(fixture
        .run(Request::List {
            root: fixture.0.clone(),
            relative: ".aws".into(),
            show_hidden: true,
            show_ignored: true
        })
        .is_err());
    let Outcome::Found { paths, .. } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "credentials".into(),
            show_hidden: true,
            show_ignored: true,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert!(
        paths.is_empty(),
        "restricted directories must not be traversed"
    );
}

#[test]
fn hidden_toggle_and_lazy_directory_listing() {
    let fixture = Fixture::new();
    fixture.write(".hidden.txt", "hidden");
    fixture.write("visible.txt", "visible");
    fixture.write("folder/deep.txt", "deep");
    let (entries, truncated) = fixture.list(false, false);
    assert!(!truncated);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "folder");
    assert!(!entries.iter().any(|e| e.relative.contains("deep.txt")));
    assert!(fixture
        .list(true, false)
        .0
        .iter()
        .any(|e| e.name == ".hidden.txt"));
}

#[test]
fn actual_git_ignore_rules_are_used_without_hiding_tracked_files() {
    let fixture = Fixture::new();
    let output = Command::new("git")
        .args(["init", "-q"])
        .arg(&fixture.0)
        .output()
        .expect("Git required by project workflow tests");
    assert!(output.status.success());
    fixture.write(".gitignore", "*.tmp\ncache/\nvendor/\n");
    fixture.write("ignored.tmp", "ignored");
    fixture.write(":(glob)literal.tmp", "literal filename");
    fixture.write("file[1].tmp", "literal brackets");
    fixture.write("tracked.tmp", "tracked");
    fixture.write("cache/deep.txt", "ignored directory");
    fixture.write("vendor/nested/keep.rs", "force-tracked source");
    fixture.write("vendor/ignored.rs", "ignored source");
    let output = Command::new("git")
        .arg("-C")
        .arg(&fixture.0)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "add",
            "-f",
            "--",
            "tracked.tmp",
            "vendor/nested/keep.rs",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let (entries, _) = fixture.list(false, false);
    assert!(entries
        .iter()
        .any(|e| e.name == "tracked.tmp" && !e.ignored));
    assert!(entries.iter().any(|e| e.name == "vendor" && !e.ignored));
    assert!(!entries.iter().any(|e| matches!(
        e.name.as_str(),
        "ignored.tmp" | "cache" | ":(glob)literal.tmp" | "file[1].tmp"
    )));
    let (entries, _) = fixture.list(false, true);
    assert!(entries.iter().any(|e| e.name == "ignored.tmp" && e.ignored));
    assert!(entries
        .iter()
        .any(|e| e.name == ":(glob)literal.tmp" && e.ignored));
    assert!(entries.iter().any(|e| e.name == "file[1].tmp" && e.ignored));
    assert!(entries.iter().any(|e| e.name == "cache" && e.ignored));
    let Outcome::Found { paths, .. } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "keep.rs".into(),
            show_hidden: false,
            show_ignored: false,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert_eq!(paths, ["vendor/nested/keep.rs"]);
    let Outcome::Found { paths, .. } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "ignored.rs".into(),
            show_hidden: false,
            show_ignored: false,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert!(paths.is_empty());
}

#[test]
fn traversal_absolute_paths_and_special_files_are_blocked() {
    let fixture = Fixture::new();
    fixture.write("plain.txt", "safe");
    for relative in [
        "../outside",
        "/etc/passwd",
        "folder/../../plain.txt",
        "./plain.txt",
        "folder\\plain.txt",
        "bad\0path",
    ] {
        assert!(
            fixture
                .run(Request::Read {
                    root: fixture.0.clone(),
                    relative: relative.into()
                })
                .is_err(),
            "{relative:?}"
        );
    }
    assert!(fixture
        .run(Request::Read {
            root: fixture.0.clone(),
            relative: String::new()
        })
        .is_err());
}

#[cfg(unix)]
#[test]
fn symlink_files_and_ancestors_are_never_followed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write("private.txt", "outside sentinel");
    symlink(outside.0.join("private.txt"), fixture.0.join("link.txt")).unwrap();
    symlink(&outside.0, fixture.0.join("linked-folder")).unwrap();
    let document = fixture.open("link.txt");
    assert_eq!(document.kind, DocumentKind::Symlink);
    assert!(document.text.is_none());
    assert!(fixture
        .save("link.txt", document.stamp, "overwrite")
        .is_err());
    assert!(fixture
        .run(Request::Read {
            root: fixture.0.clone(),
            relative: "linked-folder/private.txt".into()
        })
        .is_err());
    assert!(fixture
        .run(Request::List {
            root: fixture.0.clone(),
            relative: "linked-folder".into(),
            show_hidden: true,
            show_ignored: true
        })
        .is_err());
    let Outcome::Found { paths, .. } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "private".into(),
            show_hidden: true,
            show_ignored: true,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert!(paths.is_empty());
    assert_eq!(
        fs::read_to_string(outside.0.join("private.txt")).unwrap(),
        "outside sentinel"
    );
}

#[cfg(unix)]
#[test]
fn symlink_replacement_after_open_blocks_save() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.write("file.txt", "original");
    outside.write("outside.txt", "outside");
    let document = fixture.open("file.txt");
    fs::remove_file(fixture.0.join("file.txt")).unwrap();
    symlink(outside.0.join("outside.txt"), fixture.0.join("file.txt")).unwrap();
    assert!(fixture
        .save("file.txt", document.stamp, "new content")
        .is_err());
    assert_eq!(
        fs::read_to_string(outside.0.join("outside.txt")).unwrap(),
        "outside"
    );
}

#[cfg(unix)]
#[test]
fn atomic_save_preserves_mode_and_blocks_hard_links() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let fixture = Fixture::new();
    fixture.write("tool.sh", "original");
    fs::set_permissions(fixture.0.join("tool.sh"), fs::Permissions::from_mode(0o751)).unwrap();
    let document = fixture.open("tool.sh");
    fixture.save("tool.sh", document.stamp, "updated").unwrap();
    assert_eq!(
        fs::metadata(fixture.0.join("tool.sh")).unwrap().mode() & 0o777,
        0o751
    );
    fs::hard_link(fixture.0.join("tool.sh"), fixture.0.join("linked.sh")).unwrap();
    let document = fixture.open("tool.sh");
    assert!(fixture
        .save("tool.sh", document.stamp, "not saved")
        .unwrap_err()
        .contains("hard links"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("linked.sh")).unwrap(),
        "updated"
    );
}

#[test]
fn cancellation_and_deadline_block_before_mutation() {
    let fixture = Fixture::new();
    fixture.write("file.txt", "original");
    let document = fixture.open("file.txt");
    let result = execute(
        Request::Save {
            root: fixture.0.clone(),
            relative: "file.txt".into(),
            expected: document.stamp,
            text: "unsaved".into(),
        },
        &AtomicBool::new(true),
    );
    assert!(result.unwrap_err().contains("cancelled"));
    let cancel = AtomicBool::new(false);
    let expired = Job {
        cancel: &cancel,
        deadline: Instant::now() - Duration::from_millis(1),
    };
    assert!(expired.check().unwrap_err().contains("ten-second"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "original"
    );
}

#[test]
fn search_is_bounded_and_matches_unicode_paths() {
    let fixture = Fixture::new();
    for i in 0..SEARCH_RESULT_LIMIT + 5 {
        fixture.write(&format!("folder/match-{i:04}.txt"), "fixture");
    }
    fixture.write("nested/日本語.txt", "Unicode");
    let Outcome::Found { paths, truncated } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "match-".into(),
            show_hidden: false,
            show_ignored: false,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert_eq!(paths.len(), SEARCH_RESULT_LIMIT);
    assert!(truncated);
    let Outcome::Found { paths, truncated } = fixture
        .run(Request::Search {
            root: fixture.0.clone(),
            query: "日本語".into(),
            show_hidden: false,
            show_ignored: false,
        })
        .unwrap()
    else {
        panic!("expected search")
    };
    assert_eq!(paths, ["nested/日本語.txt"]);
    assert!(!truncated);
}

#[test]
fn large_folder_listing_reports_truncation() {
    let fixture = Fixture::new();
    for i in 0..LIST_LIMIT + 1 {
        fixture.write(&format!("file-{i:04}.txt"), "fixture");
    }
    let (entries, truncated) = fixture.list(false, true);
    assert_eq!(entries.len(), LIST_LIMIT);
    assert!(truncated);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn held_directory_listing_and_atomic_save_do_not_follow_swapped_parent() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.write("inner/inside.txt", "inside");
    outside.write("outside.txt", "outside");
    outside.write("inside.txt", "do not overwrite outside");
    let root = project_root(&fixture.0).unwrap();
    let held = anchored::directory(&root, "inner").unwrap();
    let (mut prepared, mut temporary) =
        anchored::Prepared::create(&root, "inner/inside.txt").unwrap();
    temporary.write_all(b"new inside").unwrap();
    fs::rename(fixture.0.join("inner"), fixture.0.join("moved")).unwrap();
    symlink(&outside.0, fixture.0.join("inner")).unwrap();
    let cancel = AtomicBool::new(false);
    let job = Job {
        cancel: &cancel,
        deadline: Instant::now() + JOB_TIMEOUT,
    };
    let (entries, truncated) = anchored::entries(&held, &job).unwrap();
    assert!(!truncated);
    let names: Vec<_> = entries.into_iter().map(|item| item.name).collect();
    assert!(names.iter().any(|name| name == "inside.txt"));
    assert!(!names.iter().any(|name| name == "outside.txt"));
    assert!(anchored::read(&root, "inner/inside.txt").is_err());
    prepared.replace().unwrap();
    assert_eq!(
        fs::read_to_string(fixture.0.join("moved/inside.txt")).unwrap(),
        "new inside"
    );
    assert_eq!(
        fs::read_to_string(outside.0.join("inside.txt")).unwrap(),
        "do not overwrite outside"
    );
}

#[test]
fn credential_folder_cannot_be_promoted_to_project_root() {
    let fixture = Fixture::new();
    fixture.write(".ssh/id_ed25519", "secret");
    let request = Request::List {
        root: fixture.0.join(".ssh"),
        relative: String::new(),
        show_hidden: true,
        show_ignored: true,
    };
    assert!(fixture
        .run(request)
        .unwrap_err()
        .contains("cannot be used as a project root"));
}

#[test]
fn deleted_file_is_not_recreated_by_save() {
    let fixture = Fixture::new();
    fixture.write("deleted.txt", "original");
    let document = fixture.open("deleted.txt");
    fs::remove_file(fixture.0.join("deleted.txt")).unwrap();
    assert!(fixture
        .save("deleted.txt", document.stamp, "new buffer")
        .is_err());
    assert!(!fixture.0.join("deleted.txt").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn project_root_identity_rejects_replaced_root_and_ancestor() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.write("parent/project/source.rs", "inside");
    outside.write("project/source.rs", "outside");
    let root = project_root(&fixture.0.join("parent/project")).unwrap();
    fs::rename(fixture.0.join("parent"), fixture.0.join("moved")).unwrap();
    symlink(&outside.0, fixture.0.join("parent")).unwrap();
    assert!(anchored::directory(&root, "")
        .unwrap_err()
        .contains("Project root changed"));
    assert!(anchored::read(&root, "source.rs").is_err());
    assert!(resolve(&root, "source.rs", false).is_err());
}

#[test]
fn preview_glb_returns_verified_bytes_and_rejects_stale_stamp() {
    let fixture = Fixture::new();
    let bytes = b"glTF\0disposable bytes for downstream decoder";
    fixture.write("model.glb", bytes);
    let document = fixture.open("model.glb");
    let outcome = fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: "model.glb".into(),
            expected: document.stamp.clone(),
        })
        .unwrap();
    let Outcome::PreviewGlb {
        relative,
        bytes: verified,
    } = outcome
    else {
        panic!("expected preview bytes")
    };
    assert_eq!(relative, "model.glb");
    assert_eq!(verified, bytes);
    fixture.write("model.glb", b"glTF\0changed bytes");
    assert!(fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: "model.glb".into(),
            expected: document.stamp
        })
        .unwrap_err()
        .contains("changed on disk"));
}

#[test]
fn preview_glb_supports_explicit_large_assets_but_enforces_limit() {
    let fixture = Fixture::new();
    fixture.write("model.glb", vec![1; TEXT_LIMIT as usize + 1]);
    let document = fixture.open("model.glb");
    assert_eq!(document.kind, DocumentKind::Oversized);
    let outcome = fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: "model.glb".into(),
            expected: document.stamp,
        })
        .unwrap();
    let Outcome::PreviewGlb { bytes, .. } = outcome else {
        panic!("expected preview")
    };
    assert_eq!(bytes.len(), TEXT_LIMIT as usize + 1);
    fixture.write("huge.glb", vec![1; GLB_PREVIEW_LIMIT as usize + 1]);
    let document = fixture.open("huge.glb");
    assert!(fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: "huge.glb".into(),
            expected: document.stamp
        })
        .unwrap_err()
        .contains("16 MiB"));
}

#[cfg(unix)]
#[test]
fn preview_glb_blocks_sensitive_paths_and_symlink_replacement() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.write("model.glb", b"glTF\0inside");
    outside.write("outside.glb", b"glTF\0outside");
    let document = fixture.open("model.glb");
    fs::remove_file(fixture.0.join("model.glb")).unwrap();
    symlink(outside.0.join("outside.glb"), fixture.0.join("model.glb")).unwrap();
    assert!(fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: "model.glb".into(),
            expected: document.stamp
        })
        .is_err());
    fixture.write(".env.glb", b"glTF\0restricted");
    let document = fixture.open(".env.glb");
    assert!(fixture
        .run(Request::PreviewGlb {
            root: fixture.0.clone(),
            relative: ".env.glb".into(),
            expected: document.stamp
        })
        .unwrap_err()
        .contains("Sensitive files"));
}

#[cfg(target_os = "macos")]
#[test]
fn native_mac_directory_stream_preserves_names_and_types() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fixture.write("folder/source.rs", "inside");
    fixture.write("日本語 with spaces.txt", "text");
    symlink(fixture.0.join("folder"), fixture.0.join("folder link")).unwrap();
    let (entries, truncated) = fixture.list(true, true);
    assert!(!truncated);
    assert!(entries
        .iter()
        .any(|entry| entry.name == "folder" && entry.is_dir && !entry.is_symlink));
    assert!(entries
        .iter()
        .any(|entry| entry.name == "folder link" && entry.is_symlink && !entry.is_dir));
    assert!(entries
        .iter()
        .any(|entry| entry.name == "日本語 with spaces.txt" && !entry.is_dir && !entry.is_symlink));
    assert!(!entries
        .iter()
        .any(|entry| entry.name == "." || entry.name == ".."));
}
