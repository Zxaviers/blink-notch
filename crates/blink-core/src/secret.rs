//! Feed URLs carry private access tokens, so they live in the Windows
//! Credential Manager and never in settings files, logs or the page.
//! Same service and entry names as the WebView2 app.

pub const SERVICE: &str = "deadline-notch";

pub trait Secrets: Send + Sync {
    fn get(&self, feed_id: &str) -> Option<String>;
    fn set(&self, feed_id: &str, url: &str) -> Result<(), &'static str>;
    fn delete(&self, feed_id: &str);
}

/// The real thing: Windows Credential Manager.
#[cfg(windows)]
pub struct CredentialManager;

#[cfg(windows)]
impl Secrets for CredentialManager {
    fn get(&self, feed_id: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, feed_id).ok()?.get_password().ok()
    }

    fn set(&self, feed_id: &str, url: &str) -> Result<(), &'static str> {
        keyring::Entry::new(SERVICE, feed_id)
            .map_err(|_| "STORE")?
            .set_password(url)
            .map_err(|_| "STORE")
    }

    fn delete(&self, feed_id: &str) {
        if let Ok(e) = keyring::Entry::new(SERVICE, feed_id) {
            let _ = e.delete_credential();
        }
    }
}

/// In-memory secrets for tests.
#[derive(Default)]
pub struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl Secrets for MemorySecrets {
    fn get(&self, feed_id: &str) -> Option<String> {
        self.0.lock().unwrap().get(feed_id).cloned()
    }

    fn set(&self, feed_id: &str, url: &str) -> Result<(), &'static str> {
        self.0.lock().unwrap().insert(feed_id.to_string(), url.to_string());
        Ok(())
    }

    fn delete(&self, feed_id: &str) {
        self.0.lock().unwrap().remove(feed_id);
    }
}