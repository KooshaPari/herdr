use super::command::hook_command;
use super::config_edit::{
    build_codex_config_with_hooks, ensure_command_hook, ensure_hooks_object, remove_hook_commands,
};
use super::config_file::{check_config_targets, write_config};
use super::env::codex_dir;
use super::file_ops::make_executable;
use super::types::CodexInstallPaths;
use super::{CODEX_HOOK_ASSET, CODEX_HOOK_INSTALL_NAME};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn backup_codex_files(dir: &Path) -> io::Result<PathBuf> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let backup = dir.join(format!(
        ".herdr-codex-backup-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&backup)?;
    let mut manifest = Map::new();
    for name in [CODEX_HOOK_INSTALL_NAME, "hooks.json", "config.toml"] {
        let source = dir.join(name);
        let present = match fs::metadata(&source) {
            Ok(metadata) if metadata.is_file() => {
                let destination = backup.join(name);
                let mut input = fs::File::open(&source)?;
                let mut output = crate::platform::create_config_temporary(&destination, true)?;
                io::copy(&mut input, &mut output)?;
                output.sync_all()?;
                true
            }
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} is not a regular file",
                    source.display()
                )))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        manifest.insert(name.to_owned(), Value::Bool(present));
    }
    let manifest_path = backup.join("manifest.json");
    let mut manifest_file = crate::platform::create_config_temporary(&manifest_path, true)?;
    io::Write::write_all(&mut manifest_file, &serde_json::to_vec_pretty(&manifest)?)?;
    manifest_file.sync_all()?;
    Ok(backup)
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if codex_target_is_redirect(&metadata) => {
            return Err(io::Error::other(format!(
                "refusing redirected path {}",
                path.display()
            )))
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn apply_codex_writes(
    backup: &Path,
    writes: &[(&Path, &[u8], bool)],
    mut observe: impl FnMut(usize) -> io::Result<()>,
) -> io::Result<()> {
    let mut attempted = Vec::new();
    let result = (|| {
        for (index, (path, contents, executable)) in writes.iter().enumerate() {
            let original = read_optional(
                &backup.join(
                    path.file_name()
                        .ok_or_else(|| io::Error::other("missing file name"))?,
                ),
            )?;
            if read_optional(path)? != original {
                return Err(io::Error::other(format!(
                    "{} changed after backup",
                    path.display()
                )));
            }
            let permissions = fs::metadata(path)
                .ok()
                .map(|metadata| metadata.permissions());
            attempted.push((*path, *contents, original, permissions, *executable));
            write_config(path, contents)?;
            if *executable {
                make_executable(path)?;
            }
            observe(index)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, written, original, permissions, executable) in attempted.into_iter().rev() {
            let current = match read_optional(path) {
                Ok(current) => current,
                Err(read_error) => {
                    failures.push(format!("{}: {read_error}", path.display()));
                    continue;
                }
            };
            if current == original {
                if executable {
                    if let Some(mode) = permissions {
                        if let Err(error) = fs::set_permissions(path, mode) {
                            failures.push(format!("{}: {error}", path.display()));
                        }
                    }
                }
                continue;
            }
            if current.as_deref() != Some(written) {
                failures.push(format!("{} changed or is incomplete", path.display()));
                continue;
            }
            let restored = match original {
                Some(bytes) => write_config(path, bytes).and_then(|()| {
                    if let Some(mode) = permissions {
                        fs::set_permissions(path, mode)?;
                    }
                    Ok(())
                }),
                None => fs::remove_file(path),
            };
            if let Err(rollback_error) = restored {
                failures.push(format!("{}: {rollback_error}", path.display()));
            }
        }
        if !failures.is_empty() {
            return Err(io::Error::other(format!(
                "install failed: {error}; rollback incomplete: {}; inspect backup {}",
                failures.join("; "),
                backup.display()
            )));
        }
        return Err(io::Error::other(format!(
            "install failed and changes rolled back; backup {}: {error}",
            backup.display()
        )));
    }
    Ok(())
}

pub(crate) fn install_codex() -> io::Result<CodexInstallPaths> {
    install_codex_at(codex_dir()?)
}

fn codex_target_is_redirect(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn install_codex_at(dir: PathBuf) -> io::Result<CodexInstallPaths> {
    install_codex_observed(dir, |_| Ok(()))
}

pub(crate) fn install_codex_observed(
    dir: PathBuf,
    observe: impl FnMut(usize) -> io::Result<()>,
) -> io::Result<CodexInstallPaths> {
    let marker_path = dir.join(".herdr-codex-install-in-progress");
    let marker = super::codex_marker::acquire(&marker_path).map_err(|error| {
        io::Error::other(format!(
            "cannot acquire Codex install marker {}: {error}",
            marker_path.display()
        ))
    })?;
    match install_codex_locked(dir, observe) {
        Ok(paths) => {
            marker.complete().map_err(|error| {
                io::Error::other(format!(
                    "installation completed but marker {} remains incomplete: {error}",
                    marker_path.display()
                ))
            })?;
            Ok(paths)
        }
        Err(error) => Err(io::Error::other(format!(
            "{error}; install marker remains incomplete at {}",
            marker_path.display()
        ))),
    }
}

fn install_codex_locked(
    dir: PathBuf,
    mut observe: impl FnMut(usize) -> io::Result<()>,
) -> io::Result<CodexInstallPaths> {
    for name in [CODEX_HOOK_INSTALL_NAME, "hooks.json", "config.toml"] {
        let path = dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if codex_target_is_redirect(&metadata) => {
                return Err(io::Error::other(format!(
                    "cannot install over symbolic link {}",
                    path.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    check_config_targets(
        &dir,
        &[CODEX_HOOK_INSTALL_NAME, "hooks.json", "config.toml"],
    )?;
    if !dir.is_dir() {
        return Err(io::Error::other(format!(
            "codex config directory not found at {}. install codex first",
            dir.display()
        )));
    }

    let hook_path = dir.join(CODEX_HOOK_INSTALL_NAME);
    let hooks_path = dir.join("hooks.json");
    let backup_path = backup_codex_files(&dir)?;
    let saved_hooks_path = backup_path.join("hooks.json");
    let mut hooks_file = if saved_hooks_path.is_file() {
        serde_json::from_str::<Value>(&fs::read_to_string(&saved_hooks_path)?).map_err(|err| {
            io::Error::other(format!("failed to parse {}: {err}", hooks_path.display()))
        })?
    } else {
        json!({})
    };

    let hooks = ensure_hooks_object(
        &mut hooks_file,
        &hooks_path,
        "codex hooks file",
        "codex hooks file hooks",
    )?;
    remove_hook_commands(hooks, "PermissionRequest", &hook_path, Some("blocked"))?;
    remove_hook_commands(hooks, "SessionStart", &hook_path, Some("idle"))?;
    remove_hook_commands(hooks, "UserPromptSubmit", &hook_path, Some("working"))?;
    remove_hook_commands(hooks, "PreToolUse", &hook_path, Some("working"))?;
    remove_hook_commands(hooks, "Stop", &hook_path, Some("idle"))?;
    remove_hook_commands(hooks, "SessionStart", &hook_path, Some("session"))?;
    ensure_command_hook(
        hooks,
        "SessionStart",
        hook_command(&hook_path, Some("session")),
        10,
        None,
    )?;
    let updated_hooks = serde_json::to_string_pretty(&hooks_file)?;
    let config_path = dir.join("config.toml");
    let saved_config_path = backup_path.join("config.toml");
    let existing_config = if saved_config_path.is_file() {
        fs::read_to_string(&saved_config_path)?
    } else {
        String::new()
    };
    let new_config = build_codex_config_with_hooks(&existing_config);

    observe(usize::MAX)?;
    let mut writes: Vec<(&Path, &[u8], bool)> = vec![
        (&hook_path, CODEX_HOOK_ASSET.as_bytes(), true),
        (&hooks_path, updated_hooks.as_bytes(), false),
    ];
    if new_config != existing_config {
        writes.push((&config_path, new_config.as_bytes(), false));
    }
    apply_codex_writes(&backup_path, &writes, observe)?;

    Ok(CodexInstallPaths {
        hook_path,
        hooks_path,
        config_path,
        backup_path,
    })
}
