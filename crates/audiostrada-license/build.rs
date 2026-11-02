//! Release date of the build (`AUDIOSTRADA_RELEASE_DATE`, set by the release CI, else today) and
//! public key of the licenses (`AUDIOSTRADA_LICENSE_KEY`, hex, official builds only).

include!("src/date.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=AUDIOSTRADA_RELEASE_DATE");
    println!("cargo:rerun-if-env-changed=AUDIOSTRADA_LICENSE_KEY");
    let date = std::env::var("AUDIOSTRADA_RELEASE_DATE").unwrap_or_else(|_| today());
    println!("cargo:rustc-env=AUDIOSTRADA_RELEASE_DATE={date}");
}
