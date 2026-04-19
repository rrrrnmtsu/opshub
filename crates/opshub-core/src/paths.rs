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
