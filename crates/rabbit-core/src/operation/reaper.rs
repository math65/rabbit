use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;
use crate::model::Platform;

use super::{
    PackageAutomationSupport, PlannedAutomationKind, PlannedExecutionKind,
    PlannedExecutionOverride, target_likely_portable,
};

pub(super) const TITLE: &str = "REAPER";

/// REAPER-specific automation routing. Returns `Some(verdict)` when REAPER
/// upgrades the generic planned-unattended verdict to an unattended one for
/// the given (kind, platform); returns `None` to defer to the generic
/// fallback chain.
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

pub(super) fn manual_install_notes(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let mut notes = vec![
        "REAPER application installers should be launched and completed by RABBIT itself in supported builds, but this engine slice does not execute them yet."
            .to_string(),
    ];
    if target_likely_portable(resource_path, target_app_path) {
        notes.push(format!(
            "This looks like a portable target. REAPER application files and reaper.ini should end up under {}.",
            resource_path.display()
        ));
    } else if let Some(target_app_path) = target_app_path {
        notes.push(format!(
            "This target may require administrator approval if REAPER is installed to {}.",
            reaper_install_destination(target_app_path).display()
        ));
    }
    notes
}

/// Files written by an unattended REAPER install that the receipt should
/// reference, scoped to ones that actually exist on disk after the run.
pub(super) fn receipt_paths(resource_path: &Path, target_app_path: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = target_app_path.filter(|path| path.exists()) {
        paths.push(path.to_path_buf());
        if target_likely_portable(resource_path, Some(path)) {
            let ini_path = resource_path.join("reaper.ini");
            if ini_path.exists() {
                paths.push(ini_path);
            }
        }
    }
    paths
}

pub(super) fn verification_paths(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(target_app_path) = target_app_path {
        paths.push(target_app_path.to_path_buf());
        if target_likely_portable(resource_path, Some(target_app_path)) {
            paths.push(resource_path.join("reaper.ini"));
        }
    } else {
        paths.push(resource_path.to_path_buf());
    }
    paths
}

pub(super) fn installer_arguments(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(reaper_windows_installer_arguments(
            resource_path,
            target_app_path,
        )),
        _ => None,
    }
}

pub(super) fn planned_execution_override(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Option<PlannedExecutionOverride> {
    match (kind, platform) {
        (ArtifactKind::DiskImage, Platform::MacOs) => {
            let (bundle_basename, install_destination) =
                reaper_macos_app_bundle_install_target(resource_path, target_app_path);
            Some(PlannedExecutionOverride {
                kind: PlannedExecutionKind::MountDiskImageAndCopyAppBundle,
                arguments: vec![bundle_basename, install_destination.display().to_string()],
                use_cached_working_dir: false,
            })
        }
        _ => None,
    }
}

fn reaper_windows_installer_arguments(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let install_destination = target_app_path
        .map(reaper_install_destination)
        .unwrap_or_else(|| resource_path.to_path_buf());
    let mut arguments = Vec::new();
    if target_likely_portable(resource_path, target_app_path) {
        arguments.push("/PORTABLE".to_string());
    }
    arguments.push("/S".to_string());
    arguments.push(format!("/D={}", install_destination.display()));
    arguments
}

pub(super) fn reaper_manual_steps(
    kind: ArtifactKind,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let install_destination = target_app_path.map(reaper_install_destination);
    if target_likely_portable(resource_path, target_app_path) {
        return match kind {
            ArtifactKind::Installer => vec![
                format!(
                    "In the REAPER installer, choose Portable install and use this folder: {}",
                    resource_path.display()
                ),
                format!(
                    "After installation, confirm that {} exists.",
                    resource_path.join("reaper.ini").display()
                ),
            ],
            ArtifactKind::DiskImage | ArtifactKind::Archive | ArtifactKind::SevenZipArchive => {
                vec![
                    format!(
                        "Copy REAPER into this portable folder: {}",
                        install_destination
                            .unwrap_or_else(|| resource_path.to_path_buf())
                            .display()
                    ),
                    format!(
                        "Create or keep {} for the portable resource layout.",
                        resource_path.join("reaper.ini").display()
                    ),
                ]
            }
            ArtifactKind::ExtensionBinary => vec![format!(
                "Place the REAPER application files under this target: {}",
                resource_path.display()
            )],
        };
    }

    match kind {
        ArtifactKind::Installer => {
            let destination = install_destination.unwrap_or_else(|| resource_path.to_path_buf());
            vec![
                format!(
                    "Install REAPER to this destination: {}",
                    destination.display()
                ),
                format!(
                    "After installation, start REAPER once if needed so this resource path exists: {}",
                    resource_path.display()
                ),
            ]
        }
        ArtifactKind::DiskImage | ArtifactKind::Archive | ArtifactKind::SevenZipArchive => {
            let destination = install_destination.unwrap_or_else(|| resource_path.to_path_buf());
            vec![
                format!("Copy REAPER to this destination: {}", destination.display()),
                format!(
                    "After installation, start REAPER once if needed so this resource path exists: {}",
                    resource_path.display()
                ),
            ]
        }
        ArtifactKind::ExtensionBinary => vec![format!(
            "Install REAPER for the target that uses this resource path: {}",
            resource_path.display()
        )],
    }
}

fn reaper_macos_app_bundle_install_target(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> (String, PathBuf) {
    let bundle = target_app_path
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "REAPER.app".to_string());
    let destination_dir = target_app_path
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| resource_path.to_path_buf());
    (bundle, destination_dir)
}

fn reaper_install_destination(target_app_path: &Path) -> PathBuf {
    if target_app_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        target_app_path
            .parent()
            .unwrap_or(target_app_path)
            .to_path_buf()
    } else {
        target_app_path.to_path_buf()
    }
}

/// Decides whether REAPER's Windows installer may leave a desktop shortcut,
/// and removes an unwanted one it created.
///
/// REAPER's silent (`/S`) standard installer always (re)creates a
/// `REAPER*.lnk` desktop shortcut — even on an update where the user had
/// deleted it — and there is no installer switch to suppress it. RABBIT's
/// policy: a desktop shortcut should remain only when this is a brand-new
/// STANDARD install, or when one already existed before the run. So we
/// snapshot the REAPER desktop shortcuts present *before* the installer runs,
/// then after it lands remove any REAPER shortcut that newly appeared —
/// unless it's a fresh standard install. Portable installs create no shortcut
/// (the `/PORTABLE` flag disables it), so this is a harmless no-op for them.
pub(super) struct DesktopShortcutPolicy {
    /// Desktop folders to (re)scan: the invoking user's and the all-users one.
    desktop_dirs: Vec<PathBuf>,
    /// REAPER desktop shortcuts present before the install. Never removed —
    /// only shortcuts that appear *after* and aren't in here are candidates.
    preexisting: BTreeSet<PathBuf>,
    /// When true, a freshly created shortcut is kept: a fresh standard
    /// (non-portable) install is the one case the user wants an icon for.
    keep_new_shortcut: bool,
}

impl DesktopShortcutPolicy {
    fn capture(desktop_dirs: Vec<PathBuf>, fresh_install: bool, portable: bool) -> Self {
        let preexisting = find_reaper_desktop_shortcuts(&desktop_dirs);
        Self {
            desktop_dirs,
            preexisting,
            keep_new_shortcut: fresh_install && !portable,
        }
    }

    /// Remove any REAPER desktop shortcut that appeared since capture, unless
    /// the policy keeps a freshly created one. Best-effort: returns the paths
    /// actually removed; unreadable folders / undeletable files are skipped so
    /// shortcut cleanup never fails an otherwise-successful install.
    pub(super) fn enforce(&self) -> Vec<PathBuf> {
        if self.keep_new_shortcut {
            return Vec::new();
        }
        let mut removed = Vec::new();
        for shortcut in find_reaper_desktop_shortcuts(&self.desktop_dirs) {
            if !self.preexisting.contains(&shortcut) && fs::remove_file(&shortcut).is_ok() {
                removed.push(shortcut);
            }
        }
        removed
    }
}

/// Capture the desktop-shortcut policy for a REAPER install, or `None` when it
/// doesn't apply (non-Windows, or no desktop folder resolves). Call this
/// *before* running the installer; call [`DesktopShortcutPolicy::enforce`]
/// once the install is confirmed on disk. `fresh_install` is true when the
/// plan is installing REAPER for the first time (vs. updating an existing one).
pub(super) fn capture_desktop_shortcut_policy(
    platform: Platform,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    fresh_install: bool,
) -> Option<DesktopShortcutPolicy> {
    if platform != Platform::Windows {
        return None;
    }
    let desktop_dirs = reaper_desktop_dirs();
    if desktop_dirs.is_empty() {
        return None;
    }
    let portable = target_likely_portable(resource_path, target_app_path);
    Some(DesktopShortcutPolicy::capture(
        desktop_dirs,
        fresh_install,
        portable,
    ))
}

/// Desktop folders an installer might drop a shortcut into: the invoking
/// user's desktop and the all-users (public) desktop. RABBIT elevates the
/// standard REAPER installer, which commonly targets the all-users desktop,
/// so both are scanned.
fn reaper_desktop_dirs() -> Vec<PathBuf> {
    [
        rabbit_platform::windows_user_desktop_dir(),
        rabbit_platform::windows_public_desktop_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Find REAPER desktop shortcuts (`REAPER*.lnk`, case-insensitive) across the
/// given desktop folders. The name filter plus the before/after diff in
/// [`DesktopShortcutPolicy::enforce`] keep us from ever touching a shortcut
/// the installer didn't just create.
fn find_reaper_desktop_shortcuts(desktop_dirs: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut shortcuts = BTreeSet::new();
    for dir in desktop_dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("reaper") && lower.ends_with(".lnk") {
                shortcuts.insert(path);
            }
        }
    }
    shortcuts
}

#[cfg(test)]
mod desktop_shortcut_tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{DesktopShortcutPolicy, find_reaper_desktop_shortcuts};

    fn touch(path: &Path) {
        fs::write(path, b"lnk").unwrap();
    }

    #[test]
    fn fresh_standard_install_keeps_a_new_shortcut() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        let policy = DesktopShortcutPolicy::capture(vec![desktop.clone()], true, false);
        // Installer creates the icon after capture.
        touch(&desktop.join("REAPER.lnk"));
        let removed = policy.enforce();
        assert!(removed.is_empty());
        assert!(
            desktop.join("REAPER.lnk").exists(),
            "a fresh standard install keeps the desktop icon"
        );
    }

    #[test]
    fn standard_update_removes_a_newly_created_shortcut() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        // No icon beforehand; this is an update (not fresh), standard target.
        let policy = DesktopShortcutPolicy::capture(vec![desktop.clone()], false, false);
        touch(&desktop.join("REAPER.lnk")); // installer recreated it
        let removed = policy.enforce();
        assert_eq!(removed, vec![desktop.join("REAPER.lnk")]);
        assert!(
            !desktop.join("REAPER.lnk").exists(),
            "an update with no prior icon removes the one the installer recreated"
        );
    }

    #[test]
    fn update_never_removes_a_preexisting_shortcut() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        touch(&desktop.join("REAPER.lnk")); // the user already has one
        let policy = DesktopShortcutPolicy::capture(vec![desktop.clone()], false, false);
        touch(&desktop.join("REAPER.lnk")); // installer overwrites the same path
        let removed = policy.enforce();
        assert!(removed.is_empty(), "a pre-existing icon is left untouched");
        assert!(desktop.join("REAPER.lnk").exists());
    }

    #[test]
    fn portable_install_removes_any_new_shortcut_even_when_fresh() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        // Portable + fresh: the user never wants an icon for portable.
        let policy = DesktopShortcutPolicy::capture(vec![desktop.clone()], true, true);
        touch(&desktop.join("REAPER.lnk"));
        let removed = policy.enforce();
        assert_eq!(removed.len(), 1, "portable installs never keep a new icon");
    }

    #[test]
    fn leaves_unrelated_and_preexisting_shortcuts_alone() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        touch(&desktop.join("Audacity.lnk")); // unrelated, pre-existing
        let policy = DesktopShortcutPolicy::capture(vec![desktop.clone()], false, false);
        touch(&desktop.join("REAPER.lnk")); // installer's new icon
        touch(&desktop.join("My Notes.lnk")); // unrelated new file
        let removed = policy.enforce();
        assert_eq!(removed, vec![desktop.join("REAPER.lnk")]);
        assert!(desktop.join("Audacity.lnk").exists());
        assert!(desktop.join("My Notes.lnk").exists());
    }

    #[test]
    fn matches_reaper_shortcuts_case_insensitively() {
        let dir = tempdir().unwrap();
        let desktop = dir.path().to_path_buf();
        touch(&desktop.join("reaper (x64).LNK"));
        let found = find_reaper_desktop_shortcuts(std::slice::from_ref(&desktop));
        assert!(found.contains(&desktop.join("reaper (x64).LNK")));
    }
}
