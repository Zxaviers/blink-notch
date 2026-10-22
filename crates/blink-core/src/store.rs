//! Small JSON files in the app data directory, written atomically (temp file,
//! then rename over the target) so a crash can never leave a half-written file.
//! Same folder and file names as the WebView2 app, so installs upgrade in place.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

/// Names the settings program may read and write through the pipe later.
pub const PAGE_FILES: &[&str] = &["settings", "done", "fired", "snoozed", "local_events"];

/// The folder the WebView2 app used: `%APPDATA%\dev.riski.deadlinenotch`.
pub const APP_DIR_NAME: &str = "dev.riski.deadlinenotch";

pub fn default_dir() -> Option<PathBuf> {
    // Tests (and the settings program they start) point this at a scratch folder.
    if let Some(dir) = std::env::var_os("BLINK_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(APP_DIR_NAME))
}

pub struct Store {
    dir: PathBuf,
    write_lock: Mutex<()>,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into(), write_lock: Mutex::new(()) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn read(&self, file_name: &str) -> Option<Vec<u8>> {
        fs::read(self.dir.join(file_name)).ok()
    }

    pub fn write_atomic(&self, file_name: &str, bytes: &[u8]) -> Result<(), &'static str> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        fs::create_dir_all(&self.dir).map_err(|_| "STORE")?;
        let target = self.dir.join(file_name);
        let temp = self.dir.join(format!("{file_name}.tmp"));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &target)
        })();
        result.map_err(|_| {
            let _ = fs::remove_file(&temp);
            "STORE"
        })
    }

    pub fn remove(&self, file_name: &str) {
        let _ = fs::remove_file(self.dir.join(file_name));
    }

    /// A named page file (`settings`, `done`, ...) as JSON; None when missing or corrupt.
    pub fn get_json(&self, name: &str) -> Result<Option<Value>, &'static str> {
        let file = page_file(name)?;
        Ok(self.read(&file).and_then(|b| serde_json::from_slice(&b).ok()))
    }

    pub fn set_json(&self, name: &str, value: &Value) -> Result<(), &'static str> {
        let file = page_file(name)?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|_| "STORE")?;
        self.write_atomic(&file, &bytes)
    }
}

fn page_file(name: &str) -> Result<String, &'static str> {
    PAGE_FILES.contains(&name).then(|| format!("{name}.json")).ok_or("STORE")
}