//! Small JSON files in the app data directory.
//!
//! Writes are atomic (temp file, then rename over the target), so a crash in
//! the middle of a save can never leave a half-written file behind.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

/// Names the page may read and write through `store_get` / `store_set`.
const PAGE_FILES: &[&str] = &["settings", "done", "fired", "snoozed", "local_events"];

static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|_| "STORE".to_string())?;
    fs::create_dir_all(&dir).map_err(|_| "STORE".to_string())?;
    Ok(dir)
}

pub fn read_file(app: &AppHandle, file_name: &str) -> Option<Vec<u8>> {
    fs::read(data_dir(app).ok()?.join(file_name)).ok()
}

pub fn write_atomic(app: &AppHandle, file_name: &str, bytes: &[u8]) -> Result<(), String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = data_dir(app)?;
    let target = dir.join(file_name);
    let temp = dir.join(format!("{file_name}.tmp"));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, &target)
    })();
    result.map_err(|_| {
        let _ = fs::remove_file(&temp);
        "STORE".to_string()
    })
}

pub fn remove_file(app: &AppHandle, file_name: &str) {
    if let Ok(dir) = data_dir(app) {
        let _ = fs::remove_file(dir.join(file_name));
    }
}

fn page_file_name(name: &str) -> Result<String, String> {
    if PAGE_FILES.contains(&name) {
        Ok(format!("{name}.json"))
    } else {
        Err("STORE".to_string())
    }
}

#[tauri::command]
pub fn store_get(app: AppHandle, name: String) -> Result<Option<serde_json::Value>, String> {
    let file = page_file_name(&name)?;
    Ok(read_file(&app, &file).and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

#[tauri::command]
pub fn store_set(app: AppHandle, name: String, value: serde_json::Value) -> Result<(), String> {
    let file = page_file_name(&name)?;
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| "STORE".to_string())?;
    write_atomic(&app, &file, &bytes)
}
