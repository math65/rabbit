//! Native Instruments Komplete Kontrol install probe.
//!
//! RABBIT promotes the ReaKontrol package from `recommended: false` to a
//! recommended-by-default install when Komplete Kontrol is detected on the
//! host — ReaKontrol's whole point is bridging Komplete Kontrol hardware /
//! software with REAPER, so its baseline being non-recommended only makes
//! sense for hosts that don't have Komplete Kontrol in the first place.
//!
//! Detection is intentionally cheap and conservative: we test for the
//! presence of well-known install footprints, not the version or edition of
//! Komplete Kontrol itself. The probe is callable on every platform and
//! returns `false` on platforms where Komplete Kontrol isn't supported, so
//! the wizard / CLI filter doesn't have to spread `cfg(target_os)` around.
//!
//! Platforms:
//!
//! - **Windows** — `HKLM\SOFTWARE\Native Instruments\Komplete Kontrol`.
//!   NI's installer creates this subkey under both the 64-bit and the
//!   `WOW6432Node` view; either is accepted.
//! - **macOS** — `/Applications/Native Instruments/Komplete Kontrol.app`.
//!   Komplete Kontrol's standalone application is the canonical user-facing
//!   signal; the host audio-plugin variants live under `/Library/Audio/Plug-Ins/`
//!   but are auxiliary and not probed here.
//! - **Other platforms** — always `false`.

#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(windows)]
const KOMPLETE_KONTROL_REGISTRY_SUBKEY: &str = "SOFTWARE\\Native Instruments\\Komplete Kontrol";

#[cfg(target_os = "macos")]
const KOMPLETE_KONTROL_APP_PATH: &str = "/Applications/Native Instruments/Komplete Kontrol.app";

/// `true` when Komplete Kontrol is installed for the current host. Returns
/// `false` on platforms where Komplete Kontrol isn't available.
pub fn is_komplete_kontrol_installed() -> bool {
    platform_is_komplete_kontrol_installed()
}

#[cfg(windows)]
fn platform_is_komplete_kontrol_installed() -> bool {
    crate::registry::hklm_subkey_exists(KOMPLETE_KONTROL_REGISTRY_SUBKEY)
}

#[cfg(target_os = "macos")]
fn platform_is_komplete_kontrol_installed() -> bool {
    is_komplete_kontrol_installed_under(Path::new("/"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_is_komplete_kontrol_installed() -> bool {
    false
}

/// macOS-only test seam: probe under an arbitrary filesystem root so unit
/// tests can pin a synthetic `/Applications/...` layout in a tempdir without
/// depending on the host's real `/Applications` directory.
#[cfg(target_os = "macos")]
pub fn is_komplete_kontrol_installed_under(root: &Path) -> bool {
    // Strip the leading `/` so we can join the constant onto an arbitrary
    // root (tempdir in tests, `/` in production).
    let relative = KOMPLETE_KONTROL_APP_PATH.trim_start_matches('/');
    root.join(relative).exists()
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn returns_false_when_app_bundle_is_absent() {
        let dir = tempdir().unwrap();
        assert!(!is_komplete_kontrol_installed_under(dir.path()));
    }

    #[test]
    fn returns_true_when_app_bundle_exists() {
        let dir = tempdir().unwrap();
        let app_path = dir
            .path()
            .join("Applications")
            .join("Native Instruments")
            .join("Komplete Kontrol.app");
        fs::create_dir_all(&app_path).unwrap();
        assert!(is_komplete_kontrol_installed_under(dir.path()));
    }
}
