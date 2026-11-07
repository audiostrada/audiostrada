//! `audiostrada license`: keys and licenses for the license server, and for tests.

use std::path::Path;

use anyhow::{Context, Result, bail};
use audiostrada_license::{License, Plan, from_hex, public_key, sign, to_hex, today, verify};

pub fn keygen() -> Result<()> {
    let mut secret = [0; 32];
    getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    println!("secret {}", to_hex(&secret));
    println!("public {}", to_hex(&public_key(&secret)));
    Ok(())
}

pub fn issue(
    secret_file: &Path,
    name: &str,
    email: &str,
    until: &str,
    commercial: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(secret_file)
        .with_context(|| format!("cannot read {}", secret_file.display()))?;
    let Some(secret) = from_hex(&text) else {
        bail!(
            "{} does not hold a secret key in hex",
            secret_file.display()
        );
    };
    if !is_date(until) {
        bail!("--until must be a date such as 2027-11-01");
    }
    let mut id = [0; 8];
    getrandom::fill(&mut id).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    let license = License {
        id: format!("lic_{}", to_hex(&id)),
        name: name.into(),
        email: email.into(),
        updates_until: until.into(),
        issued: today(),
        plan: if commercial {
            Plan::Commercial
        } else {
            Plan::Personal
        },
    };
    print!("{}", sign(&license, &secret));
    Ok(())
}

pub fn check(file: &Path, key: &str) -> Result<()> {
    let Some(key) = from_hex(key) else {
        bail!("--key must be a public key in hex");
    };
    let text =
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let license = verify(&text, &key)?;
    println!("id      {}", license.id);
    println!("name    {}", license.name);
    println!("email   {}", license.email);
    println!("until   {}", license.updates_until);
    println!("issued  {}", license.issued);
    println!("plan    {:?}", license.plan);
    Ok(())
}

fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        audiostrada_core::scratch::dir(&format!("license-{name}"))
    }

    #[test]
    fn dates_are_checked() {
        assert!(is_date("2027-11-01"));
        for bad in ["2027-1-01", "27-11-01", "2027/11/01", "2027-11-0x", ""] {
            assert!(!is_date(bad), "{bad}");
        }
    }

    #[test]
    fn issuing_and_checking_refuse_what_is_wrong() {
        let secret = temp("secret");
        assert!(
            issue(&secret, "A", "a@b.c", "2027-11-01", false).is_err(),
            "no file"
        );
        std::fs::write(&secret, "not hex").unwrap();
        assert!(
            issue(&secret, "A", "a@b.c", "2027-11-01", false).is_err(),
            "no key"
        );
        std::fs::write(&secret, to_hex(&[3; 32])).unwrap();
        assert!(
            issue(&secret, "A", "a@b.c", "soon", false).is_err(),
            "no date"
        );
        assert!(issue(&secret, "A", "a@b.c", "2027-11-01", true).is_ok());
        assert!(keygen().is_ok());
        // A license signed with that key checks out against its public key only.
        let license = License {
            id: "lic_1".into(),
            name: "A".into(),
            email: "a@b.c".into(),
            updates_until: "2027-11-01".into(),
            issued: "2026-11-01".into(),
            plan: Plan::Commercial,
        };
        let file = temp("license");
        std::fs::write(&file, sign(&license, &[3; 32])).unwrap();
        assert!(check(&file, &to_hex(&public_key(&[3; 32]))).is_ok());
        assert!(check(&file, &to_hex(&public_key(&[4; 32]))).is_err());
        assert!(check(&file, "zz").is_err());
        assert!(check(&temp("missing"), &to_hex(&public_key(&[3; 32]))).is_err());
        let _ = std::fs::remove_file(secret);
        let _ = std::fs::remove_file(file);
    }
}
