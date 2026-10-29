//! Until the Android Keystore store lands (docs/android-design.md §5), the Soulseek sign-in is
//! not saved on phones: signing in works for the session only.

pub use super::soulseek::Credentials;
use super::soulseek::CredentialStore;

pub struct NotSaved;

impl CredentialStore for NotSaved {
    fn load(&self) -> Option<Credentials> {
        None
    }
    fn save(&self, _creds: &Credentials) -> Result<(), String> {
        Err("Saving the sign-in isn't available on Android yet.".into())
    }
    fn delete(&self) {}
}
