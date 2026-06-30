use std::path::Path;

use rabbit_platform::{read_file_version_parts, read_file_version_string};

use crate::Result;
use crate::version::Version;

pub fn file_version(path: &Path) -> Result<Option<Version>> {
    file_version_with_probes(path, read_file_version_string, read_file_version_parts)
}

pub(crate) fn file_version_with_probes(
    path: &Path,
    read_string: fn(&Path) -> Option<String>,
    read_parts: fn(&Path) -> Option<[u32; 4]>,
) -> Result<Option<Version>> {
    // For REAPER specifically, prefer the user-facing string in StringFileInfo
    // (Windows) or Info.plist (macOS). Cockos's dev builds pack opaque numbers
    // into VS_FIXEDFILEINFO — `7.72+dev0508` shows up there as `78.193.94.133`,
    // which the parts-based formatter has no way to recover from. The friendly
    // string is what REAPER's about dialog displays, so it round-trips both
    // release (`"7.72"`) and dev (`"7.72+dev0508"`) builds unchanged.
    if is_reaper_app_path(path)
        && let Some(raw) = read_string(path)
        && let Ok(version) = Version::parse(&raw)
    {
        return Ok(Some(version));
    }

    let Some(parts) = read_parts(path) else {
        return Ok(None);
    };
    let version = version_string_for_path(path, &parts);
    Version::parse(version).map(Some)
}

fn version_string_for_path(path: &Path, parts: &[u32; 4]) -> String {
    if is_reaper_app_path(path)
        && let Some(version) = reaper_version_string_from_parts(parts)
    {
        return version;
    }

    trim_trailing_zero_parts(parts)
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn is_reaper_app_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.eq_ignore_ascii_case("reaper.exe") {
        return true;
    }
    // macOS: REAPER.app, REAPER64.app, REAPER-ARM.app, and any portable
    // `*reaper*.app` bundle the discovery layer accepts.
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".app") && lower.contains("reaper")
}

fn reaper_version_string_from_parts(parts: &[u32; 4]) -> Option<String> {
    if parts[3] != 0 || parts[1] >= 10 || parts[2] >= 10 {
        return None;
    }

    Some(format!("{}.{}{}", parts[0], parts[1], parts[2]))
}

fn trim_trailing_zero_parts(parts: &[u32; 4]) -> &[u32] {
    let mut len = parts.len();
    while len > 2 && parts[len - 1] == 0 {
        len -= 1;
    }
    &parts[..len]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{file_version_with_probes, trim_trailing_zero_parts, version_string_for_path};

    #[test]
    fn trims_trailing_zero_parts_but_keeps_major_minor() {
        assert_eq!(trim_trailing_zero_parts(&[7, 69, 0, 0]), &[7, 69]);
        assert_eq!(trim_trailing_zero_parts(&[2, 14, 0, 7]), &[2, 14, 0, 7]);
        assert_eq!(trim_trailing_zero_parts(&[1, 2, 6, 0]), &[1, 2, 6]);
    }

    #[test]
    fn normalizes_reaper_windows_fixed_file_versions() {
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 6, 9, 0]),
            "7.69"
        );
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 7, 0, 0]),
            "7.70"
        );
    }

    #[test]
    fn keeps_non_reaper_versions_in_standard_dotted_form() {
        assert_eq!(
            version_string_for_path(
                Path::new("/REAPER/UserPlugins/reaper_osara64.dll"),
                &[1, 2, 6, 0]
            ),
            "1.2.6"
        );
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 69, 0, 0]),
            "7.69"
        );
    }

    #[test]
    fn prefers_string_resource_for_reaper_dev_builds() {
        // REAPER 7.72+dev0508's VS_FIXEDFILEINFO is the opaque
        // `[78, 193, 94, 133]`, but the StringFileInfo `FileVersion` carries
        // the friendly `"7.72+dev0508"` Cockos shows in the about dialog.
        let result = file_version_with_probes(
            Path::new("/REAPER/reaper.exe"),
            |_| Some("7.72+dev0508".to_string()),
            |_| Some([78, 193, 94, 133]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.raw(), "7.72+dev0508");
    }

    #[test]
    fn falls_back_to_fixed_file_info_when_string_missing_for_reaper() {
        let result = file_version_with_probes(
            Path::new("/REAPER/reaper.exe"),
            |_| None,
            |_| Some([7, 6, 9, 0]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.raw(), "7.69");
    }

    #[test]
    fn falls_back_to_fixed_file_info_when_string_unparseable_for_reaper() {
        // A version string with no digits can't be parsed as a `Version`, so
        // we drop to the parts-based formatter rather than reporting nothing.
        let result = file_version_with_probes(
            Path::new("/REAPER/reaper.exe"),
            |_| Some("dev".to_string()),
            |_| Some([7, 7, 2, 0]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.raw(), "7.72");
    }

    #[test]
    fn does_not_apply_string_preference_outside_reaper_paths() {
        // Plugin StringFileInfo entries can carry odd vendor strings; sticking
        // with VS_FIXEDFILEINFO for non-REAPER files preserves existing
        // behavior for OSARA/SWS/ReaPack/etc.
        let result = file_version_with_probes(
            Path::new("/REAPER/UserPlugins/reaper_osara64.dll"),
            |_| Some("vendor-string".to_string()),
            |_| Some([1, 2, 6, 0]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.raw(), "1.2.6");
    }

    #[test]
    fn matches_macos_reaper_app_bundle_path() {
        let result = file_version_with_probes(
            Path::new("/Applications/REAPER.app"),
            |_| Some("7.72+dev0508".to_string()),
            |_| Some([78, 193, 94, 133]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.raw(), "7.72+dev0508");
    }
}
