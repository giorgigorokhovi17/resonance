//! Soulseek sign-in in the Android Keystore (docs/android-design.md §5): a non-exportable
//! AES-256-GCM key encrypts the username and password into an app-private file (Soulseek.kt).
//! Never in settings.json.

use jni::objects::{JString, JValue};

pub use super::soulseek::Credentials;
use super::soulseek::CredentialStore;

pub struct AndroidCredentials;

impl CredentialStore for AndroidCredentials {
    fn load(&self) -> Option<Credentials> {
        crate::smtc::with_bridge(|env, class| {
            let obj = env.call_static_method(class, "loadCredentials", "()Ljava/lang/String;", &[])?.l()?;
            if obj.is_null() {
                return Ok(None);
            }
            let text: String = env.get_string(&JString::from(obj))?.into();
            Ok(text.split_once('\n').map(|(username, password)| Credentials {
                username: username.to_string(),
                password: password.to_string(),
            }))
        })
        .flatten()
    }

    fn save(&self, creds: &Credentials) -> Result<(), String> {
        let res = crate::smtc::with_bridge(|env, class| {
            let user = env.new_string(&creds.username)?;
            let pass = env.new_string(&creds.password)?;
            let err = env
                .call_static_method(
                    class,
                    "saveCredentials",
                    "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&user), JValue::Object(&pass)],
                )?
                .l()?;
            if err.is_null() {
                Ok(Ok(()))
            } else {
                let msg: String = env.get_string(&JString::from(err))?.into();
                Ok(Err(msg))
            }
        });
        res.unwrap_or_else(|| Err("Could not save the sign-in to the Android Keystore.".into()))
    }

    fn delete(&self) {
        let _ = crate::smtc::with_bridge(|env, class| {
            env.call_static_method(class, "deleteCredentials", "()V", &[]).map(|_| ())
        });
    }
}
