use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum Site { Google, Spotify }

pub enum Outcome { Cookie(String), Cancelled, Failed(String) }

pub fn spawn(site: Site, _: PathBuf, done: impl FnOnce(Outcome) + Send + 'static) {
    std::thread::spawn(move || {
        let outcome = match crate::android::request("signIn", Some(matches!(site, Site::Spotify)), Duration::from_secs(600)) {
            Ok(Some(session)) => Outcome::Cookie(session),
            Ok(None) => Outcome::Cancelled,
            Err(error) => Outcome::Failed(error.to_string()),
        };
        done(outcome);
    });
}

pub fn forget(_: &Path) {
    crate::android::clear_sign_in();
}
