use super::codex_recovery::{install_codex_at, install_codex_observed};
#[cfg(windows)]
use super::test_support::symlink_file;
use super::{CODEX_HOOK_ASSET, CODEX_HOOK_INSTALL_NAME};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn unique_base() -> PathBuf {
    std::env::temp_dir().join(format!(
        "herdr-codex-recovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn install_codex_snapshot_preserves_recoverable_versioned_adoption() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let hook = base.join(CODEX_HOOK_INSTALL_NAME);
    let hooks = base.join("hooks.json");
    let config = base.join("config.toml");
    let old_hook = "# HERDR_INTEGRATION_ID=codex\n# HERDR_INTEGRATION_VERSION=9\nupstream";
    let old_hooks = "{\"hooks\": {}}";
    let old_config = "model = \"original\"\n";
    fs::write(&hook, old_hook).unwrap();
    fs::write(&hooks, old_hooks).unwrap();
    fs::write(&config, old_config).unwrap();
    let installed = install_codex_at(base.clone()).unwrap();
    let backup = installed.backup_path;
    assert_eq!(
        fs::read_to_string(backup.join(CODEX_HOOK_INSTALL_NAME)).unwrap(),
        old_hook
    );
    assert_eq!(
        fs::read_to_string(backup.join("hooks.json")).unwrap(),
        old_hooks
    );
    assert_eq!(
        fs::read_to_string(backup.join("config.toml")).unwrap(),
        old_config
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest[CODEX_HOOK_INSTALL_NAME], true);
    assert_eq!(manifest["hooks.json"], true);
    assert_eq!(manifest["config.toml"], true);
    assert_eq!(fs::read_to_string(&hook).unwrap(), CODEX_HOOK_ASSET);
    for name in [CODEX_HOOK_INSTALL_NAME, "hooks.json", "config.toml"] {
        fs::copy(backup.join(name), base.join(name)).unwrap();
    }
    assert_eq!(fs::read_to_string(&hook).unwrap(), old_hook);
    assert_eq!(fs::read_to_string(&hooks).unwrap(), old_hooks);
    assert_eq!(fs::read_to_string(&config).unwrap(), old_config);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_records_absent_files() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let installed = install_codex_at(base.clone()).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(installed.backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest[CODEX_HOOK_INSTALL_NAME], false);
    assert_eq!(manifest["hooks.json"], false);
    assert_eq!(manifest["config.toml"], false);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_injected_failures_restore_prior_contents() {
    for fail_after in 0..3 {
        let base = unique_base();
        fs::create_dir_all(&base).unwrap();
        let originals = [
            (CODEX_HOOK_INSTALL_NAME, "original hook"),
            ("hooks.json", "{\"hooks\":{}}"),
            ("config.toml", "model = \"original\"\n"),
        ];
        for (name, content) in originals {
            fs::write(base.join(name), content).unwrap();
        }
        let result = install_codex_observed(base.clone(), |index| {
            if index == fail_after {
                Err(std::io::Error::other("injected failure"))
            } else {
                Ok(())
            }
        });
        assert!(result.unwrap_err().to_string().contains("rolled back"));
        for (name, content) in originals {
            assert_eq!(fs::read_to_string(base.join(name)).unwrap(), content);
        }
        let _ = fs::remove_dir_all(base);
    }
}

#[test]
fn install_codex_snapshot_injected_failure_removes_only_created_files() {
    for fail_after in 0..3 {
        let base = unique_base();
        fs::create_dir_all(&base).unwrap();
        let result = install_codex_observed(base.clone(), |index| {
            if index == fail_after {
                Err(std::io::Error::other("injected failure"))
            } else {
                Ok(())
            }
        });
        assert!(result.unwrap_err().to_string().contains("rolled back"));
        for name in [CODEX_HOOK_INSTALL_NAME, "hooks.json", "config.toml"] {
            assert!(!base.join(name).exists(), "{name}");
        }
        let _ = fs::remove_dir_all(base);
    }
}

#[test]
fn install_codex_snapshot_rejects_hook_config_edit_after_snapshot() {
    for changed_name in ["hooks.json", "config.toml"] {
        let base = unique_base();
        fs::create_dir_all(&base).unwrap();
        let hook = base.join(CODEX_HOOK_INSTALL_NAME);
        fs::write(&hook, "original hook").unwrap();
        fs::write(base.join("hooks.json"), "{\"hooks\":{}}").unwrap();
        fs::write(base.join("config.toml"), "model = \"original\"\n").unwrap();
        let changed = base.join(changed_name);
        let result = install_codex_observed(base.clone(), |index| {
            if index == usize::MAX {
                fs::write(&changed, "intervening update").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&changed).unwrap(), "intervening update");
        assert_eq!(fs::read_to_string(&hook).unwrap(), "original hook");
        let _ = fs::remove_dir_all(base);
    }
}

#[test]
fn install_codex_snapshot_refuses_to_overwrite_intervening_change() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let hook = base.join(CODEX_HOOK_INSTALL_NAME);
    fs::write(&hook, "original").unwrap();
    let result = install_codex_observed(base.clone(), |index| {
        if index == usize::MAX {
            return Ok(());
        }
        fs::write(&hook, "concurrent change").unwrap();
        Err(std::io::Error::other("injected failure"))
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("changed or is incomplete"));
    assert_eq!(fs::read_to_string(&hook).unwrap(), "concurrent change");
    let _ = fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn install_codex_snapshot_restores_permissions_for_unchanged_hook() {
    use std::os::unix::fs::PermissionsExt;
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let hook = base.join(CODEX_HOOK_INSTALL_NAME);
    fs::write(&hook, CODEX_HOOK_ASSET).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o600)).unwrap();
    let result = install_codex_observed(base.clone(), |index| {
        if index == 0 {
            Err(std::io::Error::other("injected failure"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(
        fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_restores_unconflicted_files_after_conflict() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let hook = base.join(CODEX_HOOK_INSTALL_NAME);
    let hooks = base.join("hooks.json");
    fs::write(&hook, "original hook").unwrap();
    fs::write(&hooks, "{\"hooks\":{}}").unwrap();
    fs::write(base.join("config.toml"), "model = \"original\"\n").unwrap();
    let result = install_codex_observed(base.clone(), |index| {
        if index == 1 {
            fs::write(&hooks, "intervening change").unwrap();
            return Err(std::io::Error::other("injected failure"));
        }
        Ok(())
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("rollback incomplete"));
    assert_eq!(fs::read_to_string(&hooks).unwrap(), "intervening change");
    assert_eq!(fs::read_to_string(&hook).unwrap(), "original hook");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_repeats_after_success_and_blocks_prewrite_failure() {
    for invalid in [false, true] {
        let base = unique_base();
        fs::create_dir_all(&base).unwrap();
        if invalid {
            fs::write(base.join("hooks.json"), "{").unwrap();
        }
        let result = install_codex_at(base.clone());
        assert_eq!(result.is_err(), invalid);
        assert!(base.join(".herdr-codex-install-in-progress").is_file());
        let retry = install_codex_at(base.clone());
        if invalid {
            assert!(retry
                .unwrap_err()
                .to_string()
                .contains("incomplete or unrecognized"));
        } else {
            assert!(retry.is_ok());
        }
        let _ = fs::remove_dir_all(base);
    }
}

#[cfg(unix)]
#[test]
fn install_codex_snapshot_rejects_insecure_marker_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let marker = base.join(".herdr-codex-install-in-progress");
    fs::write(&marker, "HERDR-CODEX-MARKER-1:READY\n").unwrap();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(install_codex_at(base.clone()).is_err());
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        "HERDR-CODEX-MARKER-1:READY\n"
    );
    let _ = fs::remove_dir_all(base);
}

#[cfg(windows)]
#[test]
fn install_codex_snapshot_rejects_foreign_marker_without_changing_it() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let marker = base.join(".herdr-codex-install-in-progress");
    fs::write(&marker, "foreign marker").unwrap();
    assert!(install_codex_at(base.clone()).is_err());
    assert_eq!(fs::read_to_string(&marker).unwrap(), "foreign marker");
    assert!(!base.join(CODEX_HOOK_INSTALL_NAME).exists());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_lock_excludes_competing_process() {
    if let Some(path) = std::env::var_os("HERDR_TEST_LOCK_FIXTURE") {
        std::process::exit(if install_codex_at(PathBuf::from(path)).is_err() {
            0
        } else {
            1
        });
    }
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let result = install_codex_observed(base.clone(), |index| {
        if index == 0 {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "integration::codex_recovery_tests::install_codex_snapshot_lock_excludes_competing_process",
                    "--nocapture",
                ])
                .env("HERDR_TEST_LOCK_FIXTURE", &base)
                .status()
                .unwrap();
            assert!(child.success());
        }
        Ok(())
    });
    assert!(result.is_ok());
    assert!(install_codex_at(base.clone()).is_ok());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn install_codex_snapshot_crash_retains_marker_and_blocks_retry() {
    if let Some(path) = std::env::var_os("HERDR_TEST_CRASH_FIXTURE") {
        let crash_index = std::env::var("HERDR_TEST_CRASH_INDEX")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let _ = install_codex_observed(PathBuf::from(path), |index| {
            if index == crash_index {
                std::process::exit(66);
            }
            Ok(())
        });
        std::process::exit(67);
    }
    for (repeat, crash_index) in [false, true].into_iter().flat_map(|repeat| {
        [usize::MAX, 0, 1, 2]
            .into_iter()
            .map(move |index| (repeat, index))
    }) {
        let base = unique_base();
        fs::create_dir_all(&base).unwrap();
        let originals = [
            (CODEX_HOOK_INSTALL_NAME, "original hook"),
            ("hooks.json", "{\"hooks\":{}}"),
            ("config.toml", "model = \"original\"\n"),
        ];
        for (name, content) in originals {
            fs::write(base.join(name), content).unwrap();
        }
        let permissions: Vec<_> = originals
            .iter()
            .map(|(name, _)| fs::metadata(base.join(name)).unwrap().permissions())
            .collect();
        let reference = install_codex_at(base.clone()).unwrap();
        let installed: Vec<_> = originals
            .iter()
            .map(|(name, _)| fs::read(base.join(name)).unwrap())
            .collect();
        assert_eq!(installed[0], CODEX_HOOK_ASSET.as_bytes());
        for ((name, content), permissions) in originals.iter().zip(permissions) {
            fs::write(base.join(name), content).unwrap();
            fs::set_permissions(base.join(name), permissions).unwrap();
        }
        let reference_dir = base.with_extension("reference");
        if !repeat {
            fs::create_dir(&reference_dir).unwrap();
            fs::rename(
                base.join(".herdr-codex-install-in-progress"),
                reference_dir.join("marker"),
            )
            .unwrap();
            fs::rename(&reference.backup_path, reference_dir.join("backup")).unwrap();
            assert!(!base.join(".herdr-codex-install-in-progress").exists());
        }
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "integration::codex_recovery_tests::install_codex_snapshot_crash_retains_marker_and_blocks_retry",
                "--nocapture",
            ])
            .env("HERDR_TEST_CRASH_FIXTURE", &base)
            .env("HERDR_TEST_CRASH_INDEX", crash_index.to_string())
            .status()
            .unwrap();
        assert_eq!(child.code(), Some(66), "crash index {crash_index}");
        assert!(base.join(".herdr-codex-install-in-progress").is_file());
        let after_crash: Vec<_> = originals
            .iter()
            .map(|(name, _)| fs::read(base.join(name)).unwrap())
            .collect();
        for (index, ((name, original), actual)) in originals.iter().zip(&after_crash).enumerate() {
            let expected = if crash_index != usize::MAX && index <= crash_index {
                installed[index].as_slice()
            } else {
                original.as_bytes()
            };
            assert_eq!(actual, expected, "crash index {crash_index}, file {name}");
        }
        let snapshots: Vec<_> = fs::read_dir(&base)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".herdr-codex-backup-")
            })
            .collect();
        assert_eq!(snapshots.len(), if repeat { 2 } else { 1 });
        for snapshot in snapshots {
            for (name, original) in originals {
                assert_eq!(
                    fs::read_to_string(snapshot.path().join(name)).unwrap(),
                    original
                );
            }
        }
        let result = install_codex_at(base.clone());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("incomplete or unrecognized"));
        for ((name, _), expected) in originals.iter().zip(&after_crash) {
            assert_eq!(
                fs::read(base.join(name)).unwrap(),
                *expected,
                "retry changed {name}"
            );
        }
        let _ = fs::remove_dir_all(base);
        if !repeat {
            let _ = fs::remove_dir_all(reference_dir);
        }
    }
}
#[cfg(windows)]
#[test]
fn install_codex_snapshot_uses_protected_backup_acl() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    fs::write(base.join("config.toml"), "model = \"original\"\n").unwrap();
    let installed = install_codex_at(base.clone()).unwrap();
    let descriptor =
        crate::platform::test_config_backup_sddl(&installed.backup_path.join("config.toml"))
            .unwrap();
    assert!(
        descriptor.contains("D:P(A;;FA;;;SY)(A;;FA;;;OW)"),
        "{descriptor}"
    );
    let _ = fs::remove_dir_all(base);
}

#[cfg(windows)]
#[test]
fn install_codex_snapshot_rejects_dangling_hook_symlink() {
    let base = unique_base();
    fs::create_dir_all(&base).unwrap();
    let link = base.join(CODEX_HOOK_INSTALL_NAME);
    if !symlink_file(&base.join("missing-target"), &link) {
        return;
    }
    assert!(install_codex_at(base.clone()).is_err());
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    let _ = fs::remove_dir_all(base);
}
