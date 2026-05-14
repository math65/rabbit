use std::path::PathBuf;

use crate::artifact::ArtifactKind;
use crate::model::Platform;

use super::{
    PackageAutomationSupport, PlannedAutomationKind, PlannedExecutionKind, PlannedExecutionOverride,
};

pub(super) const TITLE: &str = "Surge XT";

/// Filename suffix of the inner `.pkg` we run from the mounted Surge XT
/// DMG. Plain `.pkg` keeps the matcher resilient to upstream filename
/// drift (the build prefix is `surge-xt-macOS-NIGHTLY-…` today but has
/// shifted before). The DMG only ever contains one installer pkg, so a
/// loose suffix match is unambiguous.
const MACOS_PKG_SUFFIX: &str = ".pkg";

/// Surge XT-specific automation routing. Both upstream artifacts ship
/// vendor installers we run under elevation:
/// - Windows: Inno Setup `setup.exe` (silent flags below).
/// - macOS: distribution `.pkg` wrapped in a `.dmg`, executed by
///   `/usr/sbin/installer` via [`run_pkg_installer_from_disk_image`].
pub(super) fn automation_support_for(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PackageAutomationSupport> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller),
        ),
        (ArtifactKind::DiskImage, Platform::MacOs) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::DiskImageInstall),
        ),
        _ => None,
    }
}

pub(super) fn installer_arguments(kind: ArtifactKind, platform: Platform) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(surge_xt_inno_setup_arguments()),
        _ => None,
    }
}

/// Inno Setup silent-install command line. `/VERYSILENT
/// /SUPPRESSMSGBOXES /NORESTART` is the documented unattended trio.
/// `/TYPE=full` picks the "Full installation" option (CLAP + VST3 +
/// Standalone + Effects variants + Data + Patches + Wavetables), and
/// `/ALLUSERS` forces the per-machine install scope the package ships
/// with by default (`{commoncf64}\VST3\…` and `{commonappdata}\Surge XT`).
/// `/CLOSEAPPLICATIONS` plus `/RESTARTAPPLICATIONS` lets the installer
/// shut down a running standalone Surge XT gracefully before replacing
/// the binary instead of returning an error.
fn surge_xt_inno_setup_arguments() -> Vec<String> {
    vec![
        "/VERYSILENT".to_string(),
        "/SUPPRESSMSGBOXES".to_string(),
        "/NORESTART".to_string(),
        "/CLOSEAPPLICATIONS".to_string(),
        "/RESTARTAPPLICATIONS".to_string(),
        "/TYPE=full".to_string(),
        "/ALLUSERS".to_string(),
    ]
}

pub(super) fn planned_execution_override(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PlannedExecutionOverride> {
    match (kind, platform) {
        (ArtifactKind::DiskImage, Platform::MacOs) => Some(PlannedExecutionOverride {
            kind: PlannedExecutionKind::MountDiskImageAndRunPkgInstaller,
            arguments: vec![MACOS_PKG_SUFFIX.to_string()],
            use_cached_working_dir: false,
        }),
        _ => None,
    }
}

/// Paths the post-install verification step expects to exist. We point
/// at the system VST3 bundle and the factory-data root rather than the
/// inner DLL/dylib: the bundle directory is created by the installer
/// before the inner binary lands, and the factory-data root is the
/// "no-portable-install" signal RABBIT wants every run to corroborate.
///
/// On Windows we **only** target `%CommonProgramFiles%` (the native /
/// 64-bit Common Files root, equivalent to Inno Setup's `{commoncf64}`)
/// because the Surge XT nightly only ships a win64 setup.exe. Including
/// the `(x86)` root here would require both paths to exist for
/// verification to pass — but the installer only writes to one of them,
/// so the other would always be reported as a "missing path" and the
/// run would fail post-install verification despite a healthy install.
pub(super) fn verification_paths(platform: Platform) -> Vec<PathBuf> {
    match platform {
        Platform::Windows => {
            let mut paths = Vec::new();
            if let Some(common) = rabbit_platform::windows_common_program_files_dir() {
                paths.push(
                    common
                        .join("VST3")
                        .join("Surge Synth Team")
                        .join("Surge XT.vst3"),
                );
            }
            if let Some(program_data) = rabbit_platform::windows_program_data_dir() {
                paths.push(program_data.join("Surge XT"));
            }
            paths
        }
        Platform::MacOs => vec![
            PathBuf::from("/Library/Audio/Plug-Ins/VST3/Surge XT.vst3"),
            PathBuf::from("/Library/Application Support/Surge XT"),
        ],
    }
}

/// Files the receipt should reference after a successful run, filtered
/// to the on-disk existing ones so the receipt doesn't claim ownership
/// of a path the installer didn't actually create.
pub(super) fn receipt_paths(platform: Platform) -> Vec<PathBuf> {
    verification_paths(platform)
        .into_iter()
        .filter(|path| path.exists())
        .collect()
}
