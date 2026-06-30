use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Architecture, Platform};

pub const PACKAGE_REAPER: &str = "reaper";
pub const PACKAGE_OSARA: &str = "osara";
pub const PACKAGE_SWS: &str = "sws";
pub const PACKAGE_REAPACK: &str = "reapack";
pub const PACKAGE_REAKONTROL: &str = "reakontrol";
pub const PACKAGE_JAWS_SCRIPTS: &str = "jaws-scripts";
pub const PACKAGE_FFMPEG: &str = "ffmpeg";
pub const PACKAGE_SURGE_XT: &str = "surge-xt";
pub const PACKAGE_APP2CLAP: &str = "app2clap";

pub const BUILTIN_PACKAGE_MANIFEST_ID: &str = "builtin-packages.json";
const BUILTIN_PACKAGE_MANIFEST: &str = include_str!("../embedded/packages/builtin-packages.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSpec {
    pub id: String,
    pub display_name: String,
    pub display_name_key: String,
    pub display_description_key: String,
    pub package_kind: PackageKind,
    pub required: bool,
    pub recommended: bool,
    /// Optional host-conditional escalation of `recommended`. When the named
    /// host capability is present, the package's *effective* recommended
    /// state flips to `true` regardless of the manifest baseline. Today this
    /// promotes ReaKontrol from `recommended: false` to recommended-by-
    /// default on hosts that have Komplete Kontrol installed. Resolve via
    /// [`effective_recommended`] rather than reading this field directly.
    #[serde(default)]
    pub recommended_when: Option<HostCapability>,
    /// When `true`, the wizard must show a package-specific acknowledgement
    /// page and the CLI must require an explicit `--accept-<package>-notice`
    /// flag before RABBIT stages or launches the install of this package.
    /// Used today by ReaPack to surface its donation notice; defaults to
    /// `false` for everything else.
    pub requires_user_acknowledgement: bool,
    pub supported_platforms: Vec<SupportedPlatform>,
    pub supported_architectures: Vec<Architecture>,
    pub latest_version_provider: Option<LatestVersionProvider>,
    pub artifact_provider: Option<ArtifactProvider>,
    pub detectors: Vec<PackageDetector>,
    pub install_steps: Vec<InstallStep>,
    pub uninstall_steps: Vec<UninstallStep>,
    pub backup_policy: BackupPolicy,
    pub user_plugin_prefixes: Vec<String>,
    pub user_plugin_suffixes: Vec<String>,
    /// The wizard UI group this package is listed under. See [`PackageCategory`].
    pub category: PackageCategory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub schema_version: u32,
    pub packages: Vec<EmbeddedPackageSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedPackageSpec {
    pub id: String,
    pub display_name: String,
    pub display_name_key: String,
    pub display_description_key: String,
    #[serde(default)]
    pub package_kind: PackageKind,
    #[serde(default)]
    pub required: bool,
    pub recommended: bool,
    /// See [`PackageSpec::recommended_when`].
    #[serde(default)]
    pub recommended_when: Option<HostCapability>,
    #[serde(default)]
    pub requires_user_acknowledgement: bool,
    #[serde(default = "all_supported_platforms")]
    pub supported_platforms: Vec<SupportedPlatform>,
    #[serde(default = "all_supported_architectures")]
    pub supported_architectures: Vec<Architecture>,
    pub latest_version_provider: Option<LatestVersionProvider>,
    pub artifact_provider: Option<ArtifactProvider>,
    #[serde(default)]
    pub detectors: Vec<PackageDetector>,
    #[serde(default)]
    pub install_steps: Vec<InstallStep>,
    #[serde(default)]
    pub uninstall_steps: Vec<UninstallStep>,
    #[serde(default)]
    pub backup_policy: BackupPolicy,
    pub user_plugin_prefixes: Vec<String>,
    pub user_plugin_suffixes: PlatformSuffixes,
    #[serde(default)]
    pub category: PackageCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    ReaperApp,
    UserPluginBinary,
    Keymap,
    ReapackPackage,
    /// Drop-in script files (e.g. `.jss`/`.jsb`) that a screen reader loads
    /// from a known per-user directory. Platform-gated: a package of this
    /// kind only appears in the wizard when the relevant screen reader is
    /// detected on the host (e.g. JAWS-for-REAPER scripts on Windows).
    ScreenReaderScripts,
}

impl Default for PackageKind {
    fn default() -> Self {
        Self::UserPluginBinary
    }
}

/// Which UI group a package is listed under in the wizard's package tree.
/// `Core` packages (the default) are the REAPER accessibility stack shown
/// under "Packages"; `Additional` packages are optional extras that aren't
/// tied to REAPER itself (e.g. Surge XT, app2clap) and are shown under a
/// separate "Additional software" group. Defaults to `Core` so a package
/// without a `category` in the manifest stays in the main list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PackageCategory {
    #[default]
    Core,
    Additional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportedPlatform {
    Windows,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatestVersionProvider {
    ReaperDownloadPage,
    OsaraUpdateJson,
    SwsHomePage,
    ReapackGithubRelease,
    ReakontrolGithubSnapshots,
    /// rejetto HFS file listing at `hoard.reaperaccessibility.com` for the
    /// JAWS-for-REAPER scripts; the highest-version `*.zip` in the folder
    /// wins.
    JawsForReaperScriptsHoard,
    /// Gyan.dev's `ffmpeg-release-full-shared.7z.ver` plain-text endpoint
    /// — a single line of UTF-8 with the latest stable release version
    /// of the GPL+nonfree shared Windows x64 build. We use Gyan as the
    /// canonical version source for FFmpeg because BtbN doesn't publish
    /// stable tagged releases (only rolling autobuilds), and Gyan is
    /// also winget's upstream for the FFmpeg package. The ARM64 fan-out
    /// (tordona/ffmpeg-win-arm64) generally tracks the same upstream
    /// stable; if it ever drifts, the artifact resolver picks the
    /// highest tordona tag matching `FFMPEG_SUPPORTED_MAJOR`.
    FfmpegGyanReleaseVersion,
    /// Surge XT nightly channel at
    /// `surge-synthesizer/surge` releases tag `Nightly`. The release tag
    /// is static; the rolling build identity lives in the asset filenames
    /// (`surge-xt-<platform>-NIGHTLY-<YYYY-MM-DD>-<short-sha>-…`). The
    /// parser scans the win64 setup.exe asset name and synthesizes a
    /// `Version` of the form `NIGHTLY-<YYYY-MM-DD>-<short-sha>` — the
    /// leading date numerics make `Version::cmp_lenient` a correct
    /// newer/older predicate without a dedicated comparator.
    SurgeXtNightly,
    /// app2clap rolling `snapshots` release at `jcsteh/app2clap`. Same
    /// shape as [`ReakontrolGithubSnapshots`] (same author): a static
    /// `snapshots` tag whose versioned `.zip` assets carry the build
    /// identity (`app2clap_<YYYY.M.D.build>.<shorthash>.zip`). The parser
    /// scans `assets[].name`, drops the trailing `.shorthash` segment, and
    /// keeps the highest `Version` — release dates/order are unreliable on
    /// a rolling tag, so we never trust them.
    App2clapGithubSnapshots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProvider {
    ReaperDownloadPage,
    OsaraSnapshots,
    SwsDownloadPage,
    ReapackGithubReleaseAssets,
    ReakontrolGithubSnapshots,
    /// HFS folder listing on `hoard.reaperaccessibility.com`: same listing
    /// the latest-version provider hits, but the artifact resolver also
    /// captures the file URL for download.
    JawsForReaperScriptsHoard,
    /// Per-arch fan-out for FFmpeg's shared Windows build: x64 comes
    /// from Gyan.dev's stable `ffmpeg-release-full-shared.7z`, ARM64
    /// from `github.com/tordona/ffmpeg-win-arm64` releases (matching
    /// the same FFmpeg major). Both ship `.7z` archives that drop their
    /// runtime DLLs under `bin/`; the resolver returns the right URL +
    /// version for the user's REAPER target arch.
    FfmpegSharedBuild,
    /// Per-platform artifact in the Surge XT nightly release. Windows →
    /// `surge-xt-win64-NIGHTLY-<YYYY-MM-DD>-<sha>-setup.exe`
    /// (`ArtifactKind::Installer`, Inno Setup). macOS →
    /// `surge-xt-macOS-NIGHTLY-<YYYY-MM-DD>-<sha>.dmg`
    /// (`ArtifactKind::DiskImage` wrapping a `productbuild` `.pkg`). The
    /// resolver scans the same JSON the latest-version provider reads,
    /// so both sides see the same date/sha pair.
    SurgeXtNightly,
    /// app2clap's `snapshots` release: resolves the highest-versioned
    /// `app2clap_<version>.<sha>.zip` asset (an `ArtifactKind::Archive`
    /// holding a single `app2clap.clap`). Mirrors the version provider so
    /// both sides agree on which snapshot is "latest".
    App2clapGithubSnapshots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageDetector {
    RabbitReceipt,
    UserPluginFile,
    FileVersionMetadata,
    ReapackRegistry,
    OsaraBinaryVersionString,
    /// Detect a JAWS-for-REAPER scripts install by following the
    /// `Reaper_JawsScripts` Programs-and-Features uninstall key to the
    /// vendor-installed `Uninstall.exe` and reading its StringFileInfo
    /// "FileVersion" resource. Lets RABBIT report a version even for users
    /// who installed the scripts before RABBIT existed (no receipt yet).
    JawsScriptsUninstallExe,
    /// Map an `avformat-XX.dll` filename (or its macOS `libavformat.XX.dylib`
    /// equivalent) to an FFmpeg release major version using the
    /// well-known libavformat-major → FFmpeg-major table. The DLL's own
    /// VERSIONINFO carries the libavformat version, not the FFmpeg
    /// release version, so a filename heuristic is the reliable shared
    /// signal across BtbN, Gyan.dev, and OSXExperts builds. We synthesize
    /// the detected version as `<major>.0.0` so an external install of
    /// FFmpeg N reports as Keep when the latest supported major is also
    /// N, and as Update when the user is on an older major.
    FfmpegLibavformatMajor,
    /// Detect app2clap by the presence of `app2clap.clap` in the per-user
    /// CLAP folder (`%LOCALAPPDATA%\Programs\Common\CLAP`). app2clap lives
    /// outside the REAPER resource path, so the standard `UserPluginFile`
    /// probe can't see it; this detector reports the plugin as installed
    /// with an unknown version when present (the RABBIT receipt, checked
    /// first, supplies the version when RABBIT did the install).
    App2clapClapFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    RunUpstreamInstaller,
    CopyUserPluginBinary,
    CopyKeymap,
    InstallReapackPackage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallStep {
    RemoveUserPluginBinary,
    RemoveKeymap,
    RemoveReapackPackage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupPolicy {
    None,
    BackupOverwrittenFiles,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformSuffixes {
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub macos: Vec<String>,
}

pub fn builtin_package_specs(platform: Platform) -> Vec<PackageSpec> {
    embedded_package_manifest()
        .packages
        .iter()
        .filter(|package| package.supports_platform(platform))
        .map(|package| package.to_package_spec(platform))
        .collect()
}

pub fn default_desired_package_ids() -> Vec<String> {
    default_desired_package_ids_for_host(&detect_host_capabilities())
}

/// Same as [`default_desired_package_ids`] but with an explicit host
/// snapshot, so tests can pin "Komplete Kontrol detected"/"missing" without
/// touching the real registry/filesystem. Honors `recommended_when` so a
/// host-conditional package (e.g. ReaKontrol when Komplete Kontrol is
/// detected) lands in the default desired set even though its manifest
/// baseline is `recommended: false`.
pub fn default_desired_package_ids_for_host(host: &HostCapabilities) -> Vec<String> {
    embedded_package_manifest()
        .packages
        .iter()
        .filter(|package| {
            package.recommended || package.recommended_when.is_some_and(|cap| host.has(cap))
        })
        .map(|package| package.id.clone())
        .collect()
}

/// Host-side facts the wizard / CLI consult to gate or escalate optional
/// packages. Lives in `rabbit-core` so callers (CLI, GUI) can share one
/// detection path. Probed via [`detect_host_capabilities`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostCapabilities {
    /// JAWS-for-Windows is installed for the current user. Gates the
    /// "JAWS-for-REAPER scripts" package's *visibility* via
    /// [`host_supports_package`].
    pub jaws_installed: bool,
    /// Native Instruments Komplete Kontrol is installed. Promotes
    /// ReaKontrol from `recommended: false` to recommended-by-default via
    /// [`effective_recommended`] without changing visibility.
    pub komplete_kontrol_installed: bool,
}

impl HostCapabilities {
    /// `true` iff `capability` is present on this host.
    pub fn has(&self, capability: HostCapability) -> bool {
        match capability {
            HostCapability::JawsInstalled => self.jaws_installed,
            HostCapability::KompleteKontrolInstalled => self.komplete_kontrol_installed,
        }
    }
}

/// Named host facilities a manifest entry can reference. Today this enum
/// drives [`PackageSpec::recommended_when`], but its design (one variant per
/// `HostCapabilities` field) is shared so future manifest fields can opt
/// into the same vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostCapability {
    JawsInstalled,
    KompleteKontrolInstalled,
}

/// Snapshot the runtime host facts that gate or escalate optional packages.
pub fn detect_host_capabilities() -> HostCapabilities {
    HostCapabilities {
        jaws_installed: rabbit_platform::is_jaws_installed(),
        komplete_kontrol_installed: rabbit_platform::is_komplete_kontrol_installed(),
    }
}

/// `true` when the host can meaningfully receive `spec`. Returns `false` for
/// packages whose `package_kind` requires a host facility that isn't present
/// — today only [`PackageKind::ScreenReaderScripts`] needs this filter, and
/// it's a JAWS-presence check.
pub fn host_supports_package(spec: &PackageSpec, host: &HostCapabilities) -> bool {
    match spec.package_kind {
        PackageKind::ScreenReaderScripts => host.jaws_installed,
        PackageKind::ReaperApp
        | PackageKind::UserPluginBinary
        | PackageKind::Keymap
        | PackageKind::ReapackPackage => true,
    }
}

/// Effective recommended state for `spec`, given the current host. Equals
/// `spec.recommended` unless `spec.recommended_when` names a present
/// capability, in which case it escalates to `true`. Use this everywhere
/// a "should this auto-tick / be in the default desired set?" decision is
/// made, not the raw [`PackageSpec::recommended`] field.
pub fn effective_recommended(spec: &PackageSpec, host: &HostCapabilities) -> bool {
    spec.recommended || spec.recommended_when.is_some_and(|cap| host.has(cap))
}

pub fn embedded_package_manifest() -> PackageManifest {
    parse_package_manifest(BUILTIN_PACKAGE_MANIFEST)
        .expect("embedded package manifest should parse")
}

pub fn embedded_package_manifest_source() -> &'static str {
    BUILTIN_PACKAGE_MANIFEST
}

pub fn parse_package_manifest(source: &str) -> Result<PackageManifest, serde_json::Error> {
    serde_json::from_str(source)
}

pub fn package_specs_by_id(platform: Platform) -> BTreeMap<String, PackageSpec> {
    builtin_package_specs(platform)
        .into_iter()
        .map(|spec| (spec.id.clone(), spec))
        .collect()
}

impl EmbeddedPackageSpec {
    pub fn supports_platform(&self, platform: Platform) -> bool {
        self.supported_platforms
            .iter()
            .any(|supported| supported.matches_platform(platform))
    }

    fn to_package_spec(&self, platform: Platform) -> PackageSpec {
        PackageSpec {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            display_name_key: self.display_name_key.clone(),
            display_description_key: self.display_description_key.clone(),
            package_kind: self.package_kind,
            required: self.required,
            recommended: self.recommended,
            recommended_when: self.recommended_when,
            requires_user_acknowledgement: self.requires_user_acknowledgement,
            supported_platforms: self.supported_platforms.clone(),
            supported_architectures: self.supported_architectures.clone(),
            latest_version_provider: self.latest_version_provider,
            artifact_provider: self.artifact_provider,
            detectors: self.detectors.clone(),
            install_steps: self.install_steps.clone(),
            uninstall_steps: self.uninstall_steps.clone(),
            backup_policy: self.backup_policy,
            user_plugin_prefixes: self.user_plugin_prefixes.clone(),
            user_plugin_suffixes: self.user_plugin_suffixes.for_platform(platform),
            category: self.category,
        }
    }
}

impl PackageSpec {
    pub fn supports_platform(&self, platform: Platform) -> bool {
        self.supported_platforms
            .iter()
            .any(|supported| supported.matches_platform(platform))
    }

    pub fn supports_architecture(&self, architecture: Architecture) -> bool {
        self.supported_architectures.contains(&architecture)
            || self
                .supported_architectures
                .contains(&Architecture::Universal)
    }
}

impl SupportedPlatform {
    pub fn matches_platform(self, platform: Platform) -> bool {
        matches!(
            (self, platform),
            (Self::Windows, Platform::Windows) | (Self::Macos, Platform::MacOs)
        )
    }
}

impl PlatformSuffixes {
    fn for_platform(&self, platform: Platform) -> Vec<String> {
        match platform {
            Platform::Windows => self.windows.clone(),
            Platform::MacOs => self.macos.clone(),
        }
    }
}

fn all_supported_platforms() -> Vec<SupportedPlatform> {
    vec![SupportedPlatform::Windows, SupportedPlatform::Macos]
}

fn all_supported_architectures() -> Vec<Architecture> {
    vec![
        Architecture::X86,
        Architecture::X64,
        Architecture::Arm64,
        Architecture::Arm64Ec,
        Architecture::Universal,
    ]
}

#[cfg(test)]
mod tests {
    use crate::model::{Architecture, Platform};
    use crate::package::{
        ArtifactProvider, BackupPolicy, HostCapabilities, HostCapability, InstallStep,
        LatestVersionProvider, PACKAGE_JAWS_SCRIPTS, PACKAGE_OSARA, PACKAGE_REAKONTROL,
        PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SURGE_XT, PACKAGE_SWS, PackageDetector,
        PackageKind, SupportedPlatform, builtin_package_specs,
        default_desired_package_ids_for_host, embedded_package_manifest,
        embedded_package_manifest_source, package_specs_by_id, parse_package_manifest,
    };

    #[test]
    fn parses_embedded_package_manifest() {
        let manifest = embedded_package_manifest();

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.packages.len(), 9);
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_REAPER)
        );
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_OSARA)
        );
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_REAKONTROL)
        );
        let reakontrol = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_REAKONTROL)
            .unwrap();
        assert_eq!(reakontrol.package_kind, PackageKind::UserPluginBinary);
        assert_eq!(
            reakontrol.latest_version_provider,
            Some(LatestVersionProvider::ReakontrolGithubSnapshots)
        );
        assert_eq!(
            reakontrol.artifact_provider,
            Some(ArtifactProvider::ReakontrolGithubSnapshots)
        );
        assert_eq!(reakontrol.user_plugin_prefixes, vec!["reaper_kontrol"]);
        assert!(
            reakontrol
                .install_steps
                .contains(&InstallStep::CopyUserPluginBinary)
        );
        assert!(
            reakontrol
                .detectors
                .contains(&PackageDetector::UserPluginFile)
        );
        let reaper = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_REAPER)
            .unwrap();
        assert_eq!(reaper.package_kind, PackageKind::ReaperApp);
        assert_eq!(
            reaper.latest_version_provider,
            Some(LatestVersionProvider::ReaperDownloadPage)
        );
        assert_eq!(
            reaper.artifact_provider,
            Some(ArtifactProvider::ReaperDownloadPage)
        );
        assert_eq!(reaper.backup_policy, BackupPolicy::None);
        assert!(
            reaper
                .install_steps
                .contains(&InstallStep::RunUpstreamInstaller)
        );
        let osara = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_OSARA)
            .unwrap();
        assert_eq!(osara.package_kind, PackageKind::UserPluginBinary);
        assert_eq!(
            osara.latest_version_provider,
            Some(LatestVersionProvider::OsaraUpdateJson)
        );
        assert_eq!(
            osara.artifact_provider,
            Some(ArtifactProvider::OsaraSnapshots)
        );
        assert_eq!(osara.backup_policy, BackupPolicy::BackupOverwrittenFiles);
        assert!(osara.detectors.contains(&PackageDetector::UserPluginFile));
        assert!(
            osara
                .install_steps
                .contains(&InstallStep::CopyUserPluginBinary)
        );
        assert!(embedded_package_manifest_source().contains("\"packages\""));
        let jaws = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_JAWS_SCRIPTS)
            .unwrap();
        assert_eq!(jaws.package_kind, PackageKind::ScreenReaderScripts);
        assert_eq!(jaws.supported_platforms, vec![SupportedPlatform::Windows]);
        // Manifest baseline is "not recommended"; the package escalates to
        // recommended-by-default via `recommended_when: jaws_installed` so
        // it lands in the desired set only on hosts with JAWS detected.
        assert!(!jaws.recommended);
        assert_eq!(jaws.recommended_when, Some(HostCapability::JawsInstalled));
        assert_eq!(
            jaws.latest_version_provider,
            Some(LatestVersionProvider::JawsForReaperScriptsHoard)
        );
        assert_eq!(
            jaws.artifact_provider,
            Some(ArtifactProvider::JawsForReaperScriptsHoard)
        );
        let surge = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_SURGE_XT)
            .unwrap();
        assert_eq!(surge.package_kind, PackageKind::UserPluginBinary);
        assert_eq!(
            surge.supported_platforms,
            vec![SupportedPlatform::Windows, SupportedPlatform::Macos]
        );
        assert!(!surge.recommended);
        assert_eq!(
            surge.latest_version_provider,
            Some(LatestVersionProvider::SurgeXtNightly)
        );
        assert_eq!(
            surge.artifact_provider,
            Some(ArtifactProvider::SurgeXtNightly)
        );
        assert!(
            surge
                .install_steps
                .contains(&InstallStep::RunUpstreamInstaller)
        );
        assert!(surge.detectors.contains(&PackageDetector::RabbitReceipt));
        assert!(
            surge
                .detectors
                .contains(&PackageDetector::FileVersionMetadata)
        );
        assert!(surge.user_plugin_prefixes.is_empty());
    }

    #[test]
    fn builds_platform_specific_package_specs_from_manifest() {
        let windows = package_specs_by_id(Platform::Windows);
        let macos = package_specs_by_id(Platform::MacOs);

        assert_eq!(
            windows[PACKAGE_REAPACK].user_plugin_suffixes,
            vec![".dll".to_string()]
        );
        assert_eq!(
            macos[PACKAGE_REAPACK].user_plugin_suffixes,
            vec![".dylib".to_string()]
        );
        assert_eq!(windows[PACKAGE_SWS].display_name, "SWS Extension");
        assert_eq!(
            windows[PACKAGE_SWS].package_kind,
            PackageKind::UserPluginBinary
        );
        assert!(windows[PACKAGE_SWS].supports_platform(Platform::Windows));
        assert!(windows[PACKAGE_SWS].supports_architecture(Architecture::X64));
    }

    #[test]
    fn default_desired_packages_are_recommended_manifest_packages() {
        // Pin host capabilities so the test result doesn't depend on whether
        // the dev/CI machine happens to have JAWS or Komplete Kontrol
        // installed — both gate JAWS-scripts/ReaKontrol inclusion via
        // `recommended_when`.
        let host = HostCapabilities::default();
        assert_eq!(
            default_desired_package_ids_for_host(&host),
            vec![
                PACKAGE_REAPER.to_string(),
                PACKAGE_OSARA.to_string(),
                PACKAGE_SWS.to_string(),
            ]
        );
    }

    #[test]
    fn default_desired_packages_include_jaws_scripts_when_jaws_is_detected() {
        let host = HostCapabilities {
            jaws_installed: true,
            ..HostCapabilities::default()
        };
        assert!(
            default_desired_package_ids_for_host(&host)
                .iter()
                .any(|id| id == PACKAGE_JAWS_SCRIPTS),
            "JAWS-for-REAPER scripts must escalate to a default-desired \
             package when JAWS is detected on the host"
        );
    }

    #[test]
    fn default_desired_packages_include_reakontrol_when_komplete_kontrol_is_detected() {
        let host = HostCapabilities {
            komplete_kontrol_installed: true,
            ..HostCapabilities::default()
        };
        assert!(
            default_desired_package_ids_for_host(&host)
                .iter()
                .any(|id| id == PACKAGE_REAKONTROL),
            "ReaKontrol must escalate to a default-desired package when \
             Komplete Kontrol is detected on the host"
        );
    }

    #[test]
    fn can_parse_manifest_fixtures_without_code_changes() {
        let manifest = parse_package_manifest(
            r#"{
                "schema_version": 1,
                "packages": [{
                    "id": "example",
                    "display_name": "Example",
                    "display_name_key": "package-example",
                    "display_description_key": "package-example-description",
                    "package_kind": "user_plugin_binary",
                    "required": false,
                    "recommended": false,
                    "supported_platforms": ["windows", "macos"],
                    "supported_architectures": ["x64", "universal"],
                    "latest_version_provider": "sws_home_page",
                    "artifact_provider": "sws_download_page",
                    "detectors": ["user_plugin_file"],
                    "install_steps": ["copy_user_plugin_binary"],
                    "uninstall_steps": ["remove_user_plugin_binary"],
                    "backup_policy": "backup_overwritten_files",
                    "user_plugin_prefixes": ["reaper_example"],
                    "user_plugin_suffixes": {
                        "windows": [".dll"],
                        "macos": [".dylib"]
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.packages[0].id, "example");
        assert_eq!(
            manifest.packages[0].supported_platforms,
            vec![SupportedPlatform::Windows, SupportedPlatform::Macos]
        );
        assert!(builtin_package_specs(Platform::Windows).len() >= 3);
    }

    #[test]
    fn manifest_defaults_support_older_minimal_entries() {
        let manifest = parse_package_manifest(
            r#"{
                "schema_version": 1,
                "packages": [{
                    "id": "minimal",
                    "display_name": "Minimal",
                    "display_name_key": "package-minimal",
                    "display_description_key": "package-minimal-description",
                    "recommended": false,
                    "user_plugin_prefixes": ["reaper_minimal"],
                    "user_plugin_suffixes": {
                        "windows": [".dll"],
                        "macos": [".dylib"]
                    }
                }]
            }"#,
        )
        .unwrap();

        let package = &manifest.packages[0];
        assert_eq!(package.package_kind, PackageKind::UserPluginBinary);
        assert!(!package.required);
        assert_eq!(package.backup_policy, BackupPolicy::None);
        assert!(package.supports_platform(Platform::MacOs));
    }
}
