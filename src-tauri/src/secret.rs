//! Feed URLs carry private access tokens, so they live in the Windows
//! Credential Manager and never in settings files, logs or the page.

const SERVICE: &str = "deadline-notch";

fn entry(feed_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, feed_id).map_err(|_| "STORE".to_string())
}

pub fn set(feed_id: &str, url: &str) -> Result<(), String> {
    entry(feed_id)?
        .set_password(url)
        .map_err(|_| "STORE".to_string())
}

pub fn get(feed_id: &str) -> Option<String> {
    entry(feed_id).ok()?.get_password().ok()
}

pub fn delete(feed_id: &str) {
    if let Ok(e) = entry(feed_id) {
        let _ = e.delete_credential();
    }
}
