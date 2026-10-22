//! The JSON store and the real Windows Credential Manager.

use blink_core::secret::Secrets;
use blink_core::store::Store;
use serde_json::json;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("blink-store-{tag}-{}-{n}", std::process::id()))
}

#[test]
fn json_files_round_trip_and_only_known_names_are_allowed() {
    let dir = temp_dir("rt");
    let store = Store::new(&dir); // the folder does not exist yet: it is created on the first write
    assert_eq!(store.get_json("settings").unwrap(), None);
    store.set_json("settings", &json!({ "language": "id", "n": 1 })).unwrap();
    assert_eq!(store.get_json("settings").unwrap(), Some(json!({ "language": "id", "n": 1 })));
    assert!(dir.join("settings.json").exists());
    assert!(!dir.join("settings.json.tmp").exists(), "the temp file is renamed away");

    for bad in ["../settings", "feed-x", "", "settings.json", "SETTINGS"] {
        assert_eq!(store.get_json(bad).unwrap_err(), "STORE", "{bad:?}");
        assert_eq!(store.set_json(bad, &json!({})).unwrap_err(), "STORE");
    }
}

#[test]
fn overwriting_keeps_the_old_content_if_the_new_write_cannot_happen() {
    let dir = temp_dir("atomic");
    let store = Store::new(&dir);
    store.set_json("done", &json!({ "a::1": "t" })).unwrap();
    // A directory squatting on the temp name makes the write fail before the rename.
    std::fs::create_dir(dir.join("done.json.tmp")).unwrap();
    assert_eq!(store.set_json("done", &json!({ "b::2": "t" })).unwrap_err(), "STORE");
    assert_eq!(store.get_json("done").unwrap(), Some(json!({ "a::1": "t" })));
}

#[test]
fn a_corrupt_file_reads_as_missing() {
    let dir = temp_dir("corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fired.json"), "{ not json").unwrap();
    assert_eq!(Store::new(&dir).get_json("fired").unwrap(), None);
}

#[cfg(windows)]
#[test]
fn credential_manager_round_trip() {
    use blink_core::secret::CredentialManager;
    let id = format!("blink-test-{}", std::process::id());
    let cm = CredentialManager;
    assert_eq!(cm.get(&id), None);
    let url = "https://example.edu/calendar/export_execute.php?userid=1&authtoken=SECRET";
    cm.set(&id, url).unwrap();
    assert_eq!(cm.get(&id).as_deref(), Some(url));
    cm.delete(&id);
    assert_eq!(cm.get(&id), None);
}