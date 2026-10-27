//! Records the platform the program is built for (e.g. aarch64-apple-darwin), so the updater can
//! fetch the matching build of a new release.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=RYNX_TARGET={}", target);
}
