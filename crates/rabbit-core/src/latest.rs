use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{RabbitError, Result};
use crate::hfs::{HfsListEntry, fetch_file_list, parse_get_file_list_response};
use crate::package::{
    GithubReleaseSelector, GithubReleaseSpec, PACKAGE_JAWS_SCRIPTS, VersionRule, VersionSource,
    embedded_package_manifest,
};
use crate::plan::AvailablePackage;
use crate::version::Version;
use regex::Regex;

const USER_AGENT: &str = "RABBIT/0.1 (+https://github.com/Timtam/rabbit)";

pub const OSARA_UPDATE_URL: &str = "https://osara.reaperaccessibility.com/snapshots/update.json";
/// Gyan.dev's plain-text version stamp for the latest stable
/// `ffmpeg-release-full-shared.7z`. Returns a single line of UTF-8 like
/// `8.1.1` — no JSON, no HTML scraping. We use Gyan as the canonical
/// version source for FFmpeg (and as the x64 artifact source) because
/// BtbN doesn't publish stable tagged releases — only rolling
/// autobuilds — and Gyan is also winget's upstream for FFmpeg.
pub const FFMPEG_GYAN_VERSION_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-full-shared.7z.ver";
/// `tordona/ffmpeg-win-arm64` GitHub releases — the ARM64 source for
/// FFmpeg's data-driven `http_artifact` `github_release_max_major` target.
/// Tags are plain `<major>.<minor>.<patch>` (no `n` prefix); we pick
/// the highest non-prerelease tag whose major matches
/// [`FFMPEG_SUPPORTED_MAJOR`].
pub const FFMPEG_TORDONA_ARM64_RELEASES_URL: &str =
    "https://api.github.com/repos/tordona/ffmpeg-win-arm64/releases?per_page=100";

/// FFmpeg major version that REAPER's video decoder is known to support.
/// Bump this when a new REAPER release adds support for the next FFmpeg
/// major (e.g. REAPER 7.66 added FFmpeg 8 support → pinned to `8`). The
/// detector and the latest-version provider both reference this so a
/// single bump tracks both code paths.
pub const FFMPEG_SUPPORTED_MAJOR: u64 = 8;

/// HFS root that hosts the JAWS-for-REAPER scripts archive (rejetto HFS).
pub const JAWS_FOR_REAPER_HFS_BASE: &str = "https://hoard.reaperaccessibility.com";
/// Folder under that root where the versioned `*.zip` lives. The exact folder
/// name is the only piece that needs to track upstream changes; the parser
/// itself works with any HFS listing.
pub const JAWS_FOR_REAPER_HFS_FOLDER: &str =
    "/Custom%20actions,%20Scripts%20and%20jsfx/Windows%20Scripts/JAWS%20Scripts%20by%20Snowman/";

/// Synthesize the URL we report in `RemoteData` errors so messages stay
/// stable regardless of which HTTP verb the caller used.
fn jaws_for_reaper_listing_url() -> String {
    format!(
        "{}/~/api/get_file_list?path={}",
        JAWS_FOR_REAPER_HFS_BASE.trim_end_matches('/'),
        JAWS_FOR_REAPER_HFS_FOLDER
    )
}

/// One package whose latest-version check failed, with the error rendered
/// as a display string so callers (CLI output, wizard notes) don't need to
/// keep the live error value around.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestVersionFailure {
    pub package_id: String,
    pub message: String,
}

/// Outcome of [`fetch_latest_versions`]: the versions that could be
/// determined plus a per-package record of every provider that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestVersionsReport {
    pub packages: Vec<AvailablePackage>,
    pub failures: Vec<LatestVersionFailure>,
}

/// Query every latest-version provider, tolerating per-provider failures.
///
/// A single unreachable upstream (e.g. the SWS homepage being down) must not
/// take the whole update check with it — every other package can still be
/// checked and updated. Each provider error therefore lands in
/// [`LatestVersionsReport::failures`] instead of aborting the batch; callers
/// surface those failures (CLI warning lines, wizard rows disabled with a
/// reason) and proceed with the packages that did resolve. The only hard
/// `Err` left is failing to construct the HTTP client itself, which is a
/// local environment problem that would fail every provider identically.
pub fn fetch_latest_versions() -> Result<LatestVersionsReport> {
    let client = build_http_client()?;
    let mut packages = Vec::new();
    let mut failures = Vec::new();
    match fetch_jaws_for_reaper_latest(&client) {
        Ok(version) => packages.push(AvailablePackage {
            package_id: PACKAGE_JAWS_SCRIPTS.to_string(),
            version: Some(version),
        }),
        Err(error) => failures.push(LatestVersionFailure {
            package_id: PACKAGE_JAWS_SCRIPTS.to_string(),
            message: error.to_string(),
        }),
    }
    // Data-driven packages resolved from their manifest: a `version` rule
    // (HTML / JSON / plain-text snowflakes) or a `github_release` block, with
    // the same per-package failure tolerance as the JAWS check above.
    for spec in embedded_package_manifest().packages {
        let Some(result) = resolve_manifest_version(&client, &spec) else {
            continue;
        };
        match result {
            Ok(version) => packages.push(AvailablePackage {
                package_id: spec.id.clone(),
                version: Some(version),
            }),
            Err(error) => failures.push(LatestVersionFailure {
                package_id: spec.id.clone(),
                message: error.to_string(),
            }),
        }
    }
    Ok(LatestVersionsReport { packages, failures })
}

/// Resolve a package's latest version from its manifest, if it carries a
/// data-driven source (a `version` rule or a `github_release` block).
/// Returns `None` for the JAWS-for-REAPER scripts, which resolve via the HFS
/// folder listing instead.
fn resolve_manifest_version(
    client: &Client,
    spec: &crate::package::EmbeddedPackageSpec,
) -> Option<Result<Version>> {
    if let Some(rule) = &spec.version {
        Some(resolve_version_rule(client, rule))
    } else if let Some(github_release) = &spec.github_release {
        let url = github_release_url(github_release);
        Some(
            http_get_text(client, &url)
                .and_then(|body| resolve_github_version(&body, &url, github_release)),
        )
    } else {
        None
    }
}

/// Fetch the latest version for a single package. Useful when a UI wants to
/// stream per-package results as they arrive instead of blocking on the full
/// batch.
pub fn fetch_latest_for_package(package_id: &str) -> Result<Version> {
    if package_id == PACKAGE_JAWS_SCRIPTS {
        let client = build_http_client()?;
        return fetch_jaws_for_reaper_latest(&client);
    }
    let manifest = embedded_package_manifest();
    let spec = manifest
        .packages
        .iter()
        .find(|spec| spec.id == package_id)
        .ok_or_else(|| RabbitError::RemoteData {
            url: String::new(),
            message: format!("no package named {package_id}"),
        })?;
    let client = build_http_client()?;
    // Every package now resolves its version data-driven: a `version`
    // VersionRule (REAPER/OSARA/SWS/FFmpeg) or a `github_release` block
    // (Surge XT, ReaKontrol, ReaPack, app2clap). JAWS is special-cased above.
    resolve_manifest_version(&client, spec).unwrap_or_else(|| {
        Err(RabbitError::RemoteData {
            url: String::new(),
            message: format!("no latest-version source configured for package {package_id}"),
        })
    })
}

/// POSTs the HFS listing for the JAWS-for-REAPER scripts folder and returns
/// the highest-version `*.zip` it advertises.
pub fn fetch_jaws_for_reaper_latest(client: &Client) -> Result<Version> {
    let entries = fetch_file_list(client, JAWS_FOR_REAPER_HFS_BASE, JAWS_FOR_REAPER_HFS_FOLDER)?;
    pick_jaws_for_reaper_version(&entries)
        .map(|(version, _)| version)
        .ok_or_else(|| RabbitError::RemoteData {
            url: jaws_for_reaper_listing_url(),
            message: "no versioned JAWS-for-REAPER installer in folder listing".to_string(),
        })
}

/// Resolve a data-driven [`VersionRule`]: fetch its URL and extract a version
/// via regex (HTML), a JSON pointer, or a plain-text trim. This is the
/// generic replacement for the per-package HTML/JSON/plain-text version
/// parsers (REAPER, OSARA, SWS, FFmpeg).
pub fn resolve_version_rule(client: &Client, rule: &VersionRule) -> Result<Version> {
    match rule {
        VersionRule::PlainText { url } => {
            let body = http_get_text(client, url)?;
            resolve_plaintext_version(&body, url)
        }
        VersionRule::Json { url, pointer } => {
            let body = http_get_text(client, url)?;
            resolve_json_version(&body, url, pointer)
        }
        VersionRule::Html {
            url,
            pattern,
            format,
        } => {
            let body = http_get_text(client, url)?;
            resolve_html_version(&body, url, pattern, format)
        }
    }
}

/// Plain-text-side of [`resolve_version_rule`], split out so it's unit-testable
/// on a fixture body without an HTTP fetch. Trims the body and parses it as a
/// version — covers FFmpeg's Gyan `*.ver` endpoint (a single line like `8.1.1`).
pub(crate) fn resolve_plaintext_version(body: &str, url: &str) -> Result<Version> {
    let trimmed = body.trim();
    Version::parse(trimmed).map_err(|_| RabbitError::RemoteData {
        url: url.to_string(),
        message: format!("response is not a version: {trimmed:?}"),
    })
}

/// JSON-side of [`resolve_version_rule`], split out so it's unit-testable on a
/// fixture body without an HTTP fetch. Reads the string at the RFC 6901
/// `pointer` and parses it as a version — covers OSARA's `update.json`
/// (`/version`), whose value can carry a trailing `,<gitsha>` that
/// [`Version::parse`] tolerates.
pub(crate) fn resolve_json_version(body: &str, url: &str, pointer: &str) -> Result<Version> {
    let value: Value = serde_json::from_str(body).map_err(|source| RabbitError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let raw = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| RabbitError::RemoteData {
            url: url.to_string(),
            message: format!("missing string at JSON pointer {pointer:?}"),
        })?;
    Version::parse(raw).map_err(|_| RabbitError::RemoteData {
        url: url.to_string(),
        message: format!("value at {pointer:?} is not a version: {raw:?}"),
    })
}

/// HTML-side of [`resolve_version_rule`], split out so it's unit-testable on
/// a fixture body without an HTTP fetch.
pub(crate) fn resolve_html_version(
    body: &str,
    url: &str,
    pattern: &str,
    format: &str,
) -> Result<Version> {
    let regex = Regex::new(pattern).map_err(|err| RabbitError::RemoteData {
        url: url.to_string(),
        message: format!("invalid version regex {pattern:?}: {err}"),
    })?;
    let captures = regex
        .captures(body)
        .ok_or_else(|| RabbitError::RemoteData {
            url: url.to_string(),
            message: format!("version pattern {pattern:?} did not match"),
        })?;
    let rendered = render_capture_format(format, &captures);
    Version::parse(rendered.trim()).map_err(|_| RabbitError::RemoteData {
        url: url.to_string(),
        message: format!("extracted version is invalid: {rendered:?}"),
    })
}

/// Replace `{N}` placeholders in `format` with capture group N (empty when
/// the group didn't participate). E.g. `"{1}.{2}"` over SWS's base + build.
fn render_capture_format(format: &str, captures: &regex::Captures<'_>) -> String {
    placeholder_regex()
        .replace_all(format, |slot: &regex::Captures<'_>| {
            slot[1]
                .parse::<usize>()
                .ok()
                .and_then(|index| captures.get(index))
                .map(|group| group.as_str())
                .unwrap_or("")
                .to_string()
        })
        .into_owned()
}

fn placeholder_regex() -> &'static Regex {
    static PLACEHOLDER: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PLACEHOLDER.get_or_init(|| Regex::new(r"\{(\d+)\}").expect("static placeholder regex is valid"))
}

/// Pure-data twin of [`fetch_jaws_for_reaper_latest`] for unit tests: parses
/// an HFS listing body and extracts the highest version. Lives next to the
/// extractor so the parser can be exercised without a network call.
pub fn parse_jaws_for_reaper_listing(body: &str, url: &str) -> Result<Version> {
    let entries = parse_get_file_list_response(body, url)?;
    pick_jaws_for_reaper_version(&entries)
        .map(|(version, _)| version)
        .ok_or_else(|| RabbitError::RemoteData {
            url: url.to_string(),
            message: "no versioned JAWS-for-REAPER installer in folder listing".to_string(),
        })
}

/// Walk an HFS listing and return the highest-version `*.exe`, along with
/// the file name so the artifact resolver can build a download URL. The
/// JAWS-for-REAPER scripts are distributed as a single-file Windows
/// installer executable, so we filter on `.exe` rather than archive
/// extensions.
pub(crate) fn pick_jaws_for_reaper_version(entries: &[HfsListEntry]) -> Option<(Version, String)> {
    let mut best: Option<(Version, String)> = None;
    for entry in entries {
        if entry.is_directory {
            continue;
        }
        if !entry.name.to_ascii_lowercase().ends_with(".exe") {
            continue;
        }
        let Some(version) = jaws_for_reaper_version_from_filename(&entry.name) else {
            continue;
        };
        best = Some(match best {
            Some((current_version, current_name))
                if current_version.cmp_lenient(&version).is_ge() =>
            {
                (current_version, current_name)
            }
            _ => (version, entry.name.clone()),
        });
    }
    best
}

/// Extract a version from a JAWS-for-REAPER installer filename. Accepts
/// either a dotted version (`JFRSCRIPTS_v3.18.exe` → `3.18`) or a plain
/// integer build number (`Reaper_JawsScripts_89.exe` → `89`), since the
/// upstream naming is the latter today and the dotted form has been used
/// historically. We pick the **last** digit-or-dot run in the stem so
/// prefixes/suffixes don't confuse the picker.
pub(crate) fn jaws_for_reaper_version_from_filename(name: &str) -> Option<Version> {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return None;
    }
    let stem = &name[..name.len() - 4];

    let bytes = stem.as_bytes();
    let mut last: Option<&str> = None;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii_digit() {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let mut end = cursor;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let mut candidate = &stem[start..end];
        // Trim a trailing dot so something like `3.18.` parses as `3.18`.
        while candidate.ends_with('.') {
            candidate = &candidate[..candidate.len() - 1];
        }
        if !candidate.is_empty() {
            last = Some(candidate);
        }
        cursor = end.max(start + 1);
    }

    last.and_then(|candidate| Version::parse(candidate).ok())
}

fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|source| RabbitError::Http {
            url: "client-builder".to_string(),
            source,
        })
}

fn http_get_text(client: &Client, url: &str) -> Result<String> {
    let request = crate::http::maybe_apply_github_auth(client.get(url), url);
    let response = request
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RabbitError::Http {
            url: url.to_string(),
            source,
        })?;

    response.text().map_err(|source| RabbitError::Http {
        url: url.to_string(),
        source,
    })
}

/// The GitHub API URL a [`GithubReleaseSpec`] reads — `/releases/latest`
/// for a `Latest` selector or `/releases/tags/<tag>` for a fixed/rolling
/// tag. (For app2clap this reproduces the former `APP2CLAP_GITHUB_LATEST_URL`
/// byte-for-byte, so the GitHub-auth URL match still applies.)
pub fn github_release_url(spec: &GithubReleaseSpec) -> String {
    match &spec.release {
        GithubReleaseSelector::Latest => {
            format!("https://api.github.com/repos/{}/releases/latest", spec.repo)
        }
        GithubReleaseSelector::Tag(tag) => {
            format!(
                "https://api.github.com/repos/{}/releases/tags/{}",
                spec.repo, tag
            )
        }
    }
}

/// Resolve the latest version for a data-driven GitHub-release package. For
/// `TagName` the version is the release `tag_name`; for `AssetName` it is the
/// highest `Version` across asset filenames matching the prefix/suffix (the
/// rolling-tag assets are not in version order, so we compare all of them —
/// the same logic the hand-written ReaKontrol/app2clap parsers used).
pub fn resolve_github_version(body: &str, url: &str, spec: &GithubReleaseSpec) -> Result<Version> {
    let value: Value = serde_json::from_str(body).map_err(|source| RabbitError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    match &spec.version_from {
        VersionSource::TagName { strip_v_prefix } => {
            let tag = value
                .get("tag_name")
                .and_then(Value::as_str)
                .ok_or_else(|| RabbitError::RemoteData {
                    url: url.to_string(),
                    message: "missing string field: tag_name".to_string(),
                })?;
            let tag = if *strip_v_prefix {
                tag.trim_start_matches('v')
            } else {
                tag
            };
            Version::parse(tag).map_err(|_| RabbitError::RemoteData {
                url: url.to_string(),
                message: format!("tag_name is not a version: {tag:?}"),
            })
        }
        VersionSource::AssetName {
            strip_trailing_dot_segment,
        } => {
            let assets = value
                .get("assets")
                .and_then(Value::as_array)
                .ok_or_else(|| RabbitError::RemoteData {
                    url: url.to_string(),
                    message: "missing array field: assets".to_string(),
                })?;
            let mut latest: Option<Version> = None;
            for asset in assets {
                let Some(name) = asset.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let Some(version) =
                    github_asset_version(name, &spec.assets, *strip_trailing_dot_segment)
                else {
                    continue;
                };
                latest = Some(match latest {
                    Some(current) if current.cmp_lenient(&version).is_ge() => current,
                    _ => version,
                });
            }
            latest.ok_or_else(|| RabbitError::RemoteData {
                url: url.to_string(),
                message: "no asset matched any selector's prefix/suffix".to_string(),
            })
        }
    }
}

/// The [`Version`] of `asset_name`, extracted via the first prefix/suffix
/// [`AssetSelector`] it matches. Used by both the version side (which scans
/// every asset across all platforms) and the artifact side (which already
/// knows the matched selector). Returns `None` for assets that match no
/// prefix/suffix selector — e.g. `exact_name`-only selectors, which pair
/// with `VersionSource::TagName` rather than `AssetName`.
pub(crate) fn github_asset_version(
    asset_name: &str,
    selectors: &[crate::package::AssetSelector],
    strip_trailing_dot_segment: bool,
) -> Option<Version> {
    let selector = selectors
        .iter()
        .find(|selector| selector.matches_asset(asset_name))?;
    let prefix = selector.name_prefix.as_deref()?;
    let suffix = selector.name_suffix.as_deref().unwrap_or("");
    version_from_asset_name(asset_name, prefix, suffix, strip_trailing_dot_segment)
}

/// Extract a [`Version`] from an asset filename of the form
/// `<prefix><version>[.<trailing>]<suffix>`, e.g.
/// `app2clap_2026.5.17.34.b6f558cf.zip` → `2026.5.17.34` (prefix
/// `app2clap_`, suffix `.zip`, trailing `.<shorthash>` dropped). Returns
/// `None` when the name doesn't match. This is the parameterized form of
/// the former `app2clap_version_from_asset_name` / `reakontrol_version_from_asset_name`.
pub(crate) fn version_from_asset_name(
    name: &str,
    prefix: &str,
    suffix: &str,
    strip_trailing_dot_segment: bool,
) -> Option<Version> {
    let core = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    let token = if strip_trailing_dot_segment {
        core.rsplit_once('.').map(|(left, _trailing)| left)?
    } else {
        core
    };
    Version::parse(token).ok()
}

/// Walk the tordona/ffmpeg-win-arm64 releases JSON and return both the
/// highest stable tag whose major matches `FFMPEG_SUPPORTED_MAJOR` and
/// its assets. The ARM64 artifact resolver uses the assets list to
/// pick `ffmpeg-<ver>-full-shared-win-arm64.7z`. Pre-releases (the
/// daily `daily-autobuild-*` autobuilds and the `latest` rolling tag)
/// and majors other than the supported one are skipped.
pub(crate) fn pick_ffmpeg_tordona_release(
    body: &str,
    url: &str,
    supported_major: u64,
) -> Result<Option<TordonaRelease>> {
    let releases = parse_tordona_releases_array(body, url)?;
    let mut best: Option<TordonaRelease> = None;
    for release in releases {
        let parts = release.version.numeric_parts();
        if parts.first().copied() != Some(supported_major) {
            continue;
        }
        best = Some(match best {
            Some(current) if current.version.cmp_lenient(&release.version).is_ge() => current,
            _ => release.clone(),
        });
    }
    Ok(best)
}

#[derive(Debug, Clone)]
pub(crate) struct TordonaRelease {
    pub version: Version,
    pub assets: Vec<TordonaAsset>,
}

#[derive(Debug, Clone)]
pub(crate) struct TordonaAsset {
    pub name: String,
    pub url: String,
}

fn parse_tordona_releases_array(body: &str, url: &str) -> Result<Vec<TordonaRelease>> {
    let value: Value = serde_json::from_str(body).map_err(|source| RabbitError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let array = value.as_array().ok_or_else(|| RabbitError::RemoteData {
        url: url.to_string(),
        message: "tordona/ffmpeg-win-arm64 releases response was not a JSON array".to_string(),
    })?;

    let mut releases = Vec::with_capacity(array.len());
    for entry in array {
        if entry.get("prerelease").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(tag_name) = entry.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(version) = ffmpeg_version_from_tordona_tag(tag_name) else {
            continue;
        };
        let assets = entry
            .get("assets")
            .and_then(Value::as_array)
            .map(|assets| {
                assets
                    .iter()
                    .filter_map(|asset| {
                        let name = asset.get("name").and_then(Value::as_str)?.to_string();
                        let url = asset
                            .get("browser_download_url")
                            .and_then(Value::as_str)?
                            .to_string();
                        Some(TordonaAsset { name, url })
                    })
                    .collect()
            })
            .unwrap_or_default();
        releases.push(TordonaRelease { version, assets });
    }
    Ok(releases)
}

/// Extract a `Version` from a tordona/ffmpeg-win-arm64 release tag.
/// Tags are plain `<major>.<minor>.<patch>` (no `n` prefix, no `v`).
/// Rolling tags (`latest`, `daily-autobuild-…`) return `None`.
pub(crate) fn ffmpeg_version_from_tordona_tag(tag_name: &str) -> Option<Version> {
    if !tag_name.starts_with(|ch: char| ch.is_ascii_digit()) {
        return None;
    }
    Version::parse(tag_name).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        FFMPEG_GYAN_VERSION_URL, FFMPEG_TORDONA_ARM64_RELEASES_URL, OSARA_UPDATE_URL,
        ffmpeg_version_from_tordona_tag, github_release_url, jaws_for_reaper_listing_url,
        jaws_for_reaper_version_from_filename, parse_jaws_for_reaper_listing,
        pick_ffmpeg_tordona_release, resolve_github_version, resolve_json_version,
        resolve_plaintext_version, version_from_asset_name,
    };
    use crate::package::{
        AssetSelector, GithubArtifactKind, GithubReleaseSelector, GithubReleaseSpec,
        InstallDestination, SupportedPlatform, VersionSource,
    };

    /// The app2clap manifest's `github_release` block, mirrored as a literal
    /// for the data-driven engine tests (asset-name versioning on a rolling
    /// `snapshots` tag; the prefix/suffix live on the asset selector).
    fn app2clap_github_spec() -> GithubReleaseSpec {
        GithubReleaseSpec {
            repo: "jcsteh/app2clap".to_string(),
            release: GithubReleaseSelector::Tag("snapshots".to_string()),
            version_from: VersionSource::AssetName {
                strip_trailing_dot_segment: true,
            },
            assets: vec![AssetSelector {
                platform: SupportedPlatform::Windows,
                arch: None,
                name_prefix: Some("app2clap_".to_string()),
                name_suffix: Some(".zip".to_string()),
                exact_name: None,
                artifact_kind: None,
            }],
            artifact_kind: Some(GithubArtifactKind::Archive),
            install_destination: InstallDestination::WindowsClapDir,
        }
    }

    #[test]
    fn resolves_json_version_from_osara_update_json() {
        // OSARA's update.json `/version` carries a trailing `,<gitsha>` that
        // Version::parse keeps verbatim.
        let version = resolve_json_version(
            r#"{"version":"2026.4.16.2157,593ff26b"}"#,
            OSARA_UPDATE_URL,
            "/version",
        )
        .unwrap();
        assert_eq!(version.raw(), "2026.4.16.2157,593ff26b");
        // A missing pointer and a non-version value are errors, not panics.
        assert!(resolve_json_version(r#"{"nope":"x"}"#, OSARA_UPDATE_URL, "/version").is_err());
        assert!(
            resolve_json_version(
                r#"{"version":"not-a-version"}"#,
                OSARA_UPDATE_URL,
                "/version"
            )
            .is_err()
        );
    }

    #[test]
    fn resolve_html_version_drives_reaper_and_sws_rules() {
        // REAPER: single-group "Version X.YZ" rule (format defaults to {1}).
        let reaper = super::resolve_html_version(
            "<div class='hdrbottom'>Version 7.76: June 29, 2026</div>",
            "https://www.reaper.fm/download.php",
            r"Version ([0-9][0-9.]*)",
            "{1}",
        )
        .unwrap();
        assert_eq!(reaper.raw(), "7.76");

        // SWS: two-group base + #build combined via the format template —
        // proves the generic engine reproduces the old base.#build parser.
        let sws = super::resolve_html_version(
            "Latest stable version: v2.14.0 (build #7) released 2026-01-01",
            "https://sws-extension.org/",
            r"Latest stable version:[\s\S]*?v([0-9.]+)[\s\S]*?#([0-9]+)",
            "{1}.{2}",
        )
        .unwrap();
        assert_eq!(sws.raw(), "2.14.0.7");

        // No match is an error, not a panic.
        assert!(
            super::resolve_html_version("nothing here", "u", r"Version ([0-9.]+)", "{1}").is_err()
        );
    }

    /// ReaKontrol's `github_release` block: per-platform asset prefixes,
    /// version from the asset name. Proves the engine handles a multi-prefix
    /// package (the case that motivated selector-based version extraction).
    fn reakontrol_github_spec() -> GithubReleaseSpec {
        GithubReleaseSpec {
            repo: "jcsteh/reaKontrol".to_string(),
            release: GithubReleaseSelector::Latest,
            version_from: VersionSource::AssetName {
                strip_trailing_dot_segment: true,
            },
            assets: vec![
                AssetSelector {
                    platform: SupportedPlatform::Windows,
                    arch: None,
                    name_prefix: Some("reaKontrol_windows_".to_string()),
                    name_suffix: Some(".zip".to_string()),
                    exact_name: None,
                    artifact_kind: None,
                },
                AssetSelector {
                    platform: SupportedPlatform::Macos,
                    arch: None,
                    name_prefix: Some("reaKontrol_mac_".to_string()),
                    name_suffix: Some(".zip".to_string()),
                    exact_name: None,
                    artifact_kind: None,
                },
            ],
            artifact_kind: Some(GithubArtifactKind::Archive),
            install_destination: InstallDestination::UserPlugins,
        }
    }

    #[test]
    fn resolve_github_version_picks_highest_reakontrol_across_platform_prefixes() {
        // Windows and macOS assets carry different prefixes; the engine must
        // version each via its matching selector and keep the global highest.
        let body = r#"{
            "tag_name": "v0",
            "assets": [
                {"name": "reaKontrol_windows_2025.6.6.7.bfbe7606.zip"},
                {"name": "reaKontrol_mac_2026.2.16.100.cafef00d.zip"},
                {"name": "reaKontrol_windows_2026.2.16.100.cafef00d.zip"},
                {"name": "reaKontrol_mac_2025.7.25.10.4ce6b01f.zip"}
            ]
        }"#;
        let spec = reakontrol_github_spec();
        let version = resolve_github_version(body, &github_release_url(&spec), &spec).unwrap();
        assert_eq!(version.raw(), "2026.2.16.100");
        assert_eq!(
            github_release_url(&spec),
            "https://api.github.com/repos/jcsteh/reaKontrol/releases/latest"
        );
    }

    #[test]
    fn version_from_asset_name_strips_prefix_suffix_and_trailing_segment() {
        let version = version_from_asset_name(
            "app2clap_2026.5.17.34.b6f558cf.zip",
            "app2clap_",
            ".zip",
            true,
        )
        .unwrap();
        assert_eq!(version.raw(), "2026.5.17.34");
        let version = version_from_asset_name(
            "app2clap_2025.9.12.2.487c00a3.zip",
            "app2clap_",
            ".zip",
            true,
        )
        .unwrap();
        assert_eq!(version.raw(), "2025.9.12.2");
        assert!(version_from_asset_name("README.md", "app2clap_", ".zip", true).is_none());
        assert!(
            version_from_asset_name(
                "reaKontrol_windows_2025.6.6.7.x.zip",
                "app2clap_",
                ".zip",
                true
            )
            .is_none()
        );
        // Without the trailing-segment drop, the whole core is the version.
        let version =
            version_from_asset_name("ffmpeg_8.1.1.zip", "ffmpeg_", ".zip", false).unwrap();
        assert_eq!(version.raw(), "8.1.1");
    }

    #[test]
    fn resolve_github_version_picks_highest_app2clap_snapshot_unordered() {
        // Real `snapshots` releases list assets out of version order, so the
        // engine must compare every match rather than trust position.
        let body = r#"{
            "tag_name": "snapshots",
            "assets": [
                {"name": "app2clap_2025.11.27.30.ca402c1b.zip"},
                {"name": "app2clap_2025.9.12.2.487c00a3.zip"},
                {"name": "app2clap_2026.5.17.34.b6f558cf.zip"},
                {"name": "app2clap_2026.5.16.31.5d1e4007.zip"}
            ]
        }"#;
        let spec = app2clap_github_spec();
        let version = resolve_github_version(body, &github_release_url(&spec), &spec).unwrap();
        assert_eq!(version.raw(), "2026.5.17.34");
    }

    #[test]
    fn app2clap_github_release_url_matches_the_former_constant() {
        // Parity: the engine must build the exact endpoint the old
        // APP2CLAP_GITHUB_LATEST_URL const used, so GitHub auth still applies.
        assert_eq!(
            github_release_url(&app2clap_github_spec()),
            "https://api.github.com/repos/jcsteh/app2clap/releases/tags/snapshots"
        );
    }

    #[test]
    fn resolve_github_version_rejects_release_with_no_matching_assets() {
        let body = r#"{"tag_name": "snapshots", "assets": [{"name": "README.md"}]}"#;
        let spec = app2clap_github_spec();
        let error = resolve_github_version(body, &github_release_url(&spec), &spec).unwrap_err();
        assert!(error.to_string().contains("no asset matched"));
    }

    #[test]
    fn resolve_github_version_reads_tag_name_with_v_prefix_stripped() {
        // The ReaPack-style version source: version comes from the tag, not
        // an asset name. Proves the engine generalizes beyond app2clap.
        let body = r#"{"tag_name": "v1.2.6", "assets": []}"#;
        let spec = GithubReleaseSpec {
            repo: "cfillion/reapack".to_string(),
            release: GithubReleaseSelector::Latest,
            version_from: VersionSource::TagName {
                strip_v_prefix: true,
            },
            assets: Vec::new(),
            artifact_kind: Some(crate::package::GithubArtifactKind::ExtensionBinary),
            install_destination: crate::package::InstallDestination::UserPlugins,
        };
        let version = resolve_github_version(body, &github_release_url(&spec), &spec).unwrap();
        assert_eq!(version.raw(), "1.2.6");
        assert_eq!(
            github_release_url(&spec),
            "https://api.github.com/repos/cfillion/reapack/releases/latest"
        );
    }

    #[test]
    fn extracts_jaws_for_reaper_version_from_common_filenames() {
        let cases = [
            // Current upstream naming (single-integer build number).
            ("Reaper_JawsScripts_89.exe", "89"),
            // Historic / hypothetical dotted forms — kept covered so a
            // future rename to a semver-shaped scheme keeps working.
            ("JFRSCRIPTS_v3.18.exe", "3.18"),
            ("JFR_v3.18.0.exe", "3.18.0"),
            ("jaws-for-reaper-3.18.exe", "3.18"),
            ("JAWS_FOR_REAPER_3.18.0_release.exe", "3.18.0"),
        ];
        for (file_name, expected) in cases {
            let version = jaws_for_reaper_version_from_filename(file_name).unwrap();
            assert_eq!(version.raw(), expected, "filename: {file_name}");
        }
        assert!(jaws_for_reaper_version_from_filename("README.txt").is_none());
        assert!(jaws_for_reaper_version_from_filename("NoVersionHere.exe").is_none());
        // Non-.exe artifacts (e.g. a zip sibling) are ignored.
        assert!(jaws_for_reaper_version_from_filename("JFR_v3.18.zip").is_none());
    }

    #[test]
    fn picks_highest_jaws_for_reaper_version_from_hfs_listing() {
        let body = r#"{
            "list": [
                {"n": "Reaper_JawsScripts_88.exe", "s": 100},
                {"n": "Reaper_JawsScripts_89.exe", "s": 110},
                {"n": "Reaper_JawsScripts_85.exe", "s": 90},
                {"n": "old/", "s": null},
                {"n": "README.txt", "s": 5}
            ]
        }"#;
        let version = parse_jaws_for_reaper_listing(body, &jaws_for_reaper_listing_url()).unwrap();
        assert_eq!(version.raw(), "89");
    }

    #[test]
    fn rejects_jaws_for_reaper_listing_without_versioned_installer() {
        let body = r#"{"list": [{"n": "README.txt", "s": 1}]}"#;
        let error =
            parse_jaws_for_reaper_listing(body, &jaws_for_reaper_listing_url()).unwrap_err();
        assert!(error.to_string().contains("no versioned JAWS-for-REAPER"));
    }

    #[test]
    fn resolves_plaintext_version_from_gyan_ver_payload() {
        assert_eq!(
            resolve_plaintext_version("8.1.1\n", FFMPEG_GYAN_VERSION_URL)
                .unwrap()
                .raw(),
            "8.1.1"
        );
        assert_eq!(
            resolve_plaintext_version("  8.1.1  ", FFMPEG_GYAN_VERSION_URL)
                .unwrap()
                .raw(),
            "8.1.1"
        );
        assert!(resolve_plaintext_version("", FFMPEG_GYAN_VERSION_URL).is_err());
        assert!(resolve_plaintext_version("not-a-version", FFMPEG_GYAN_VERSION_URL).is_err());
    }

    #[test]
    fn extracts_ffmpeg_version_from_tordona_release_tag() {
        // tordona ships plain `<major>.<minor>.<patch>` tags.
        assert_eq!(
            ffmpeg_version_from_tordona_tag("8.1.1").unwrap().raw(),
            "8.1.1"
        );
        assert_eq!(
            ffmpeg_version_from_tordona_tag("7.1.4").unwrap().raw(),
            "7.1.4"
        );
        // Rolling tags and BtbN-style `n` prefixes are rejected.
        assert!(ffmpeg_version_from_tordona_tag("latest").is_none());
        assert!(ffmpeg_version_from_tordona_tag("daily-autobuild-2026.05.06.0").is_none());
        assert!(ffmpeg_version_from_tordona_tag("n8.1.1").is_none());
    }

    #[test]
    fn picks_highest_stable_n8_release_from_tordona_listing() {
        let body = r#"[
            {
                "tag_name": "daily-autobuild-2026.05.06.0",
                "prerelease": false,
                "assets": [
                    {
                        "name": "ffmpeg-master-latest-full-shared-win-arm64.7z",
                        "browser_download_url": "https://example.test/ffmpeg-master-latest-full-shared-win-arm64.7z"
                    }
                ]
            },
            {
                "tag_name": "7.1.4",
                "prerelease": false,
                "assets": []
            },
            {
                "tag_name": "8.0.2",
                "prerelease": false,
                "assets": []
            },
            {
                "tag_name": "8.1.1",
                "prerelease": false,
                "assets": [
                    {
                        "name": "ffmpeg-8.1.1-full-shared-win-arm64.7z",
                        "browser_download_url": "https://example.test/ffmpeg-8.1.1-full-shared-win-arm64.7z"
                    },
                    {
                        "name": "ffmpeg-8.1.1-full-static-win-arm64.7z",
                        "browser_download_url": "https://example.test/ffmpeg-8.1.1-full-static-win-arm64.7z"
                    }
                ]
            },
            {
                "tag_name": "9.0",
                "prerelease": true,
                "assets": []
            }
        ]"#;
        let release = pick_ffmpeg_tordona_release(body, FFMPEG_TORDONA_ARM64_RELEASES_URL, 8)
            .unwrap()
            .expect("an n8.x.y release should be selected");
        assert_eq!(release.version.raw(), "8.1.1");
        // The full-shared asset must remain in the picked release so the
        // artifact resolver can grab the `.browser_download_url`.
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == "ffmpeg-8.1.1-full-shared-win-arm64.7z")
            .expect("full-shared asset should still be carried through");
        assert_eq!(
            asset.url,
            "https://example.test/ffmpeg-8.1.1-full-shared-win-arm64.7z"
        );
    }

    #[test]
    fn errors_when_tordona_listing_has_no_supported_major() {
        let body = r#"[
            {"tag_name": "7.1.4", "prerelease": false, "assets": []},
            {"tag_name": "latest", "prerelease": true, "assets": []}
        ]"#;
        let release =
            pick_ffmpeg_tordona_release(body, FFMPEG_TORDONA_ARM64_RELEASES_URL, 8).unwrap();
        assert!(release.is_none());
    }
}
