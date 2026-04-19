use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn data_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "opshub", "opshub")
        .context("cannot determine platform data dir")?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("db.sqlite"))
}

/// User-editable agent profile directory. We intentionally use XDG-style
/// `$XDG_CONFIG_HOME/opshub/agents` (defaulting to `~/.config/opshub/agents`)
/// on every platform, ignoring the macOS `~/Library/Application Support`
/// convention — dotfile-friendly paths are what power users want, and
/// keeping one layout simplifies docs.
///
/// Created lazily; callers that only *read* should treat a missing dir as
/// "no user profiles installed" rather than an error.
pub fn user_agents_dir() -> Result<PathBuf> {
    let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            PathBuf::from(xdg)
        } else {
            default_config_base()?
        }
    } else {
        default_config_base()?
    };
    Ok(base.join("opshub").join("agents"))
}

fn default_config_base() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME env var is not set")?;
    Ok(PathBuf::from(home).join(".config"))
}
