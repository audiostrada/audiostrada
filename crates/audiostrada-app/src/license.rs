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
