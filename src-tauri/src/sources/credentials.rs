//! Soulseek sign-in details in Windows Credential Manager (a generic credential, encrypted with
//! the Windows login). Never in settings.json.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

pub use super::soulseek::Credentials;
use super::soulseek::CredentialStore;

/// The Soulseek sign-in in Credential Manager, under `target_name`.
pub struct WindowsCredentials {
    pub target: String,
}

impl CredentialStore for WindowsCredentials {
    fn load(&self) -> Option<Credentials> {
        load(&self.target)
    }
    fn save(&self, creds: &Credentials) -> Result<(), String> {
        save(&self.target, creds)
    }
    fn delete(&self) {
        delete(&self.target)
    }
}

/// Credential name. Builds run under another identifier (test runs) get their own entry.
pub fn target_name(identifier: &str) -> String {
    if identifier == "app.resonance.player" {
        "Resonance/Soulseek".to_string()
    } else {
        format!("Resonance/Soulseek/{identifier}")
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn save(target: &str, creds: &Credentials) -> Result<(), String> {
    let mut target_w = wide(target);
    let mut user_w = wide(&creds.username);
    let mut blob = creds.password.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_w.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: every pointer in `cred` points into a buffer that outlives the call.
    let res = unsafe { CredWriteW(&cred, 0) };
    blob.fill(0);
    res.map_err(|e| format!("Could not save the sign-in to Windows Credential Manager: {e}"))
}

pub fn load(target: &str) -> Option<Credentials> {
    let target_w = wide(target);
    let mut out: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success `out` points to a CREDENTIALW owned by Windows until CredFree.
    unsafe {
        CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None, &mut out).ok()?;
        let cred = &*out;
        let blob = if cred.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
        };
        let password = String::from_utf8(blob.to_vec()).ok();
        let username = if cred.UserName.is_null() { None } else { cred.UserName.to_string().ok() };
        CredFree(out as *const _);
        Some(Credentials { username: username?, password: password? })
    }
}

pub fn delete(target: &str) {
    let target_w = wide(target);
    // SAFETY: plain call with a NUL-terminated string. A missing entry is fine.
    let _ = unsafe { CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_names() {
        assert_eq!(target_name("app.resonance.player"), "Resonance/Soulseek");
        assert_eq!(target_name("app.resonance.player.visualtest"), "Resonance/Soulseek/app.resonance.player.visualtest");
    }

    #[test]
    fn round_trip_in_credential_manager() {
        let target = format!("Resonance/Soulseek/unit-test-{}", std::process::id());
        save(&target, &Credentials { username: "tester".into(), password: "pä55 word".into() }).unwrap();
        let c = load(&target).expect("saved credential");
        assert_eq!(c.username, "tester");
        assert_eq!(c.password, "pä55 word");
        delete(&target);
        assert!(load(&target).is_none());
    }
}
