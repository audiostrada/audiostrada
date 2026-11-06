//! Licenses of the official builds. A license is a JSON payload signed with Ed25519 and checked
//! offline: no account, no machine limit. It covers every version released up to
//! `updates_until`, forever. Builds from source need none.

use std::fmt;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

mod date;
pub use date::today;

/// Built by the release CI: without a license, the build runs in evaluation mode.
pub const OFFICIAL: bool = cfg!(feature = "official");
/// Release date of this build (YYYY-MM-DD).
pub const RELEASE_DATE: &str = env!("AUDIOSTRADA_RELEASE_DATE");
/// Public key the licenses are checked against (hex), given to official builds.
const PUBLIC_KEY: Option<&str> = option_env!("AUDIOSTRADA_LICENSE_KEY");

/// Web site of the official builds; every link of the program goes through it.
pub const SITE: &str = "https://audiostrada.com";

pub fn link(page: &str) -> String {
    format!("{SITE}/{page}")
}

const BEGIN: &str = "-----BEGIN AUDIOSTRADA LICENSE-----";
const END: &str = "-----END AUDIOSTRADA LICENSE-----";
const LINE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    pub id: String,
    pub name: String,
    pub email: String,
    /// Last day of the paid updates (YYYY-MM-DD).
    pub updates_until: String,
    /// Day the license was issued (YYYY-MM-DD).
    pub issued: String,
    /// What the license was bought for; licenses issued before plans existed are personal.
    #[serde(default)]
    pub plan: Plan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    /// Individuals, and businesses earning less than 20,000 € a year from audio.
    #[default]
    Personal,
    Commercial,
}

impl License {
    /// The version released on `release_date` comes within the paid updates.
    pub fn covers(&self, release_date: &str) -> bool {
        release_date <= self.updates_until.as_str()
    }

    /// A license given for life: its updates never end.
    pub fn is_lifetime(&self) -> bool {
        self.updates_until.as_str() >= "9999"
    }
}

#[derive(Debug)]
pub enum LicenseError {
    /// Not a license (truncated, or not copied whole).
    Format,
    /// Altered, or signed by someone else.
    Signature,
    /// This build has no key to check licenses with.
    NoKey,
    Io(std::io::Error),
}

impl fmt::Display for LicenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format => write!(f, "this text is not a complete license"),
            Self::Signature => write!(f, "this license is not valid"),
            Self::NoKey => write!(f, "this build cannot check licenses"),
            Self::Io(e) => write!(f, "cannot save the license: {e}"),
        }
    }
}

impl std::error::Error for LicenseError {}

/// What this copy of the program may do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Built from source: nothing to unlock.
    Unofficial,
    Licensed(License),
    /// The paid updates ended before this version was released.
    Lapsed(License),
    Evaluation,
}

impl Status {
    pub fn evaluation(&self) -> bool {
        matches!(self, Self::Lapsed(_) | Self::Evaluation)
    }

    pub fn license(&self) -> Option<&License> {
        match self {
            Self::Licensed(license) | Self::Lapsed(license) => Some(license),
            Self::Unofficial | Self::Evaluation => None,
        }
    }
}

/// Status of this build with `license`.
pub fn status(license: Option<License>) -> Status {
    if !OFFICIAL {
        return Status::Unofficial;
    }
    match license {
        Some(license) if license.covers(RELEASE_DATE) => Status::Licensed(license),
        Some(license) => Status::Lapsed(license),
        None => Status::Evaluation,
    }
}

/// Signs `license` into the text sent to the buyer.
pub fn sign(license: &License, secret: &[u8; 32]) -> String {
    let payload = serde_json::to_vec(license).unwrap_or_default();
    let signature = SigningKey::from_bytes(secret).sign(&payload);
    let body = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    );
    let mut text = format!("{BEGIN}\n");
    for line in body.as_bytes().chunks(LINE) {
        text.push_str(&String::from_utf8_lossy(line));
        text.push('\n');
    }
    text.push_str(END);
    text.push('\n');
    text
}

/// Checks a license text, however it was wrapped or indented by the mail it came in.
pub fn verify(text: &str, key: &[u8; 32]) -> Result<License, LicenseError> {
    let body: String = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("-----"))
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .collect();
    let (payload, signature) = body.split_once('.').ok_or(LicenseError::Format)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| LicenseError::Format)?;
    let signature: [u8; 64] = URL_SAFE_NO_PAD
        .decode(signature)
        .ok()
        .and_then(|s| s.try_into().ok())
        .ok_or(LicenseError::Format)?;
    VerifyingKey::from_bytes(key)
        .map_err(|_| LicenseError::NoKey)?
        .verify_strict(&payload, &Signature::from_bytes(&signature))
        .map_err(|_| LicenseError::Signature)?;
    serde_json::from_slice(&payload).map_err(|_| LicenseError::Format)
}

/// Public key of a secret signing key.
pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

/// The key official builds check licenses against.
pub fn official_key() -> Option<[u8; 32]> {
    PUBLIC_KEY.and_then(from_hex)
}

/// Where the license is kept.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("license.txt"))
}

/// The saved license, if it is valid.
pub fn load() -> Option<License> {
    let key = official_key()?;
    let text = std::fs::read_to_string(path()?).ok()?;
    verify(&text, &key).ok()
}

/// Checks and saves a license pasted or opened by the user.
pub fn install(text: &str) -> Result<License, LicenseError> {
    let key = official_key().ok_or(LicenseError::NoKey)?;
    let license = verify(text, &key)?;
    let path = path().ok_or(LicenseError::NoKey)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(LicenseError::Io)?;
    }
    std::fs::write(&path, text.trim().to_owned() + "\n").map_err(LicenseError::Io)?;
    Ok(license)
}

/// Forgets the saved license.
pub fn remove() -> std::io::Result<()> {
    match path() {
        Some(path) if path.exists() => std::fs::remove_file(path),
        _ => Ok(()),
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [7; 32];

    fn license(until: &str) -> License {
        License {
            id: "lic_1".into(),
            name: "Ada Lovelace".into(),
            email: "ada@example.org".into(),
            updates_until: until.into(),
            issued: "2026-11-01".into(),
            plan: Plan::Personal,
        }
    }

    #[test]
    fn a_signed_license_checks_out() {
        let text = sign(&license("2027-11-01"), &SECRET);
        assert!(text.starts_with(BEGIN));
        let checked = verify(&text, &public_key(&SECRET)).ok();
        assert_eq!(checked, Some(license("2027-11-01")));
    }

    #[test]
    fn mail_wrapping_does_not_matter() {
        let text = sign(&license("2027-11-01"), &SECRET);
        let quoted: String = text.lines().map(|l| format!("  > {l}  \r\n")).collect();
        let quoted = quoted.replace("  > ", "  ");
        assert!(verify(&quoted, &public_key(&SECRET)).is_ok());
        let bare: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
        assert!(verify(&bare, &public_key(&SECRET)).is_ok());
    }

    #[test]
    fn altered_or_foreign_licenses_are_refused() {
        let text = sign(&license("2027-11-01"), &SECRET);
        let body: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
        let (_, signature) = body.split_once('.').unwrap_or_default();
        let payload = serde_json::to_vec(&license("2099-12-31")).unwrap_or_default();
        let forged = format!("{}.{signature}", URL_SAFE_NO_PAD.encode(payload));
        assert!(matches!(
            verify(&forged, &public_key(&SECRET)),
            Err(LicenseError::Signature)
        ));
        assert!(matches!(
            verify(&text, &public_key(&[8; 32])),
            Err(LicenseError::Signature)
        ));
        assert!(matches!(
            verify("hello", &public_key(&SECRET)),
            Err(LicenseError::Format)
        ));
    }

    #[test]
    fn versions_released_after_the_updates_are_not_covered() {
        let license = license("2027-11-01");
        assert!(license.covers("2027-11-01"));
        assert!(license.covers("2026-12-24"));
        assert!(!license.covers("2027-11-02"));
    }

    #[test]
    fn builds_from_source_need_no_license() {
        if !OFFICIAL {
            assert_eq!(status(None), Status::Unofficial);
            assert!(!status(None).evaluation());
        }
    }

    #[test]
    fn plans_are_signed_and_default_to_personal() {
        let commercial = License {
            plan: Plan::Commercial,
            ..license("2027-11-01")
        };
        let text = sign(&commercial, &SECRET);
        assert_eq!(
            verify(&text, &public_key(&SECRET)).ok().map(|l| l.plan),
            Some(Plan::Commercial)
        );
        let old: Option<License> = serde_json::from_str(
            r#"{"id":"a","name":"b","email":"c","updates_until":"2027-01-01","issued":"2026-01-01"}"#,
        )
        .ok();
        assert_eq!(old.map(|l| l.plan), Some(Plan::Personal));
    }

    #[test]
    fn lifetime_licenses_cover_every_version() {
        let life = license("9999-12-31");
        assert!(life.is_lifetime());
        assert!(life.covers("2099-01-01"));
        assert!(!license("2027-11-01").is_lifetime());
        let status = Status::Licensed(life.clone());
        assert!(!status.evaluation());
        assert_eq!(status.license(), Some(&life));
        assert!(Status::Lapsed(life).evaluation());
        assert!(Status::Evaluation.evaluation());
        assert_eq!(Status::Evaluation.license(), None);
    }

    #[test]
    fn links_go_to_the_web_site() {
        assert_eq!(link("download"), format!("{SITE}/download"));
        assert!(link("releases/latest.json").ends_with("/releases/latest.json"));
    }

    #[test]
    fn broken_texts_are_refused_as_such() {
        let key = public_key(&SECRET);
        for text in ["", "no dot here", "a.b", "!!!.???", "e30.AAAA"] {
            assert!(verify(text, &key).is_err(), "{text:?}");
        }
        // A good signature over something that is not a license.
        let text = sign(&license("2027-11-01"), &SECRET);
        assert!(verify(&text, &[0; 32]).is_err(), "no such key");
        assert!(from_hex(&"ab".repeat(31)).is_none(), "too short");
        assert!(from_hex(&"ab".repeat(33)).is_none(), "too long");
    }

    #[test]
    fn hex_round_trips() {
        let key = public_key(&SECRET);
        assert_eq!(from_hex(&to_hex(&key)), Some(key));
        assert_eq!(from_hex("zz"), None);
    }
}
