//! Embeds the release Android APK into the host binary.
//!
//! CI sets `WATCHDOG_APK` (plus metadata variables) before building the host;
//! a developer can do the same to test a local APK. Without the variable the
//! build stays APK-free and provisioning uses the `--apk` override instead.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    for var in [
        "WATCHDOG_APK",
        "WATCHDOG_APK_VERSION_NAME",
        "WATCHDOG_APK_VERSION_CODE",
        "WATCHDOG_APK_SHA256",
        "WATCHDOG_APK_SIGNER_SHA256",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let generated = match env::var("WATCHDOG_APK") {
        Ok(path) if !path.trim().is_empty() => {
            let source = Path::new(&path);
            if !source.is_file() {
                panic!(
                    "WATCHDOG_APK points to a missing file: {}",
                    source.display()
                );
            }
            fs::copy(source, out_dir.join("watchdog.apk"))
                .unwrap_or_else(|e| panic!("cannot copy {}: {e}", source.display()));
            let version_name = env::var("WATCHDOG_APK_VERSION_NAME").unwrap_or_default();
            let version_code = env::var("WATCHDOG_APK_VERSION_CODE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "0".to_string());
            let sha256 = env::var("WATCHDOG_APK_SHA256").unwrap_or_default();
            let signer = env::var("WATCHDOG_APK_SIGNER_SHA256").unwrap_or_default();
            format!(
                "/// Release APK embedded by the build script.\n\
                 pub const EMBEDDED: Option<EmbeddedApk> = Some(EmbeddedApk {{\n\
                 \x20   bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/watchdog.apk\")),\n\
                 \x20   version_name: {version_name:?},\n\
                 \x20   version_code: {version_code},\n\
                 \x20   sha256: {sha256:?},\n\
                 \x20   signer_sha256: {signer:?},\n\
                 }});\n"
            )
        }
        _ => "/// No APK was embedded in this build.\n\
              pub const EMBEDDED: Option<EmbeddedApk> = None;\n"
            .to_string(),
    };
    fs::write(out_dir.join("embedded_apk.rs"), generated).expect("write embedded_apk.rs");
}
