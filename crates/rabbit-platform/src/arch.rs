//! Host-CPU architecture probes that the cross-platform engine can't get
//! from Rust's `target_arch` cfg alone — namely Rosetta detection on
//! Apple Silicon Macs.

/// Returns `true` when the current process is being translated by macOS
/// Rosetta (an `x86_64` binary running on an Apple Silicon host).
///
/// Used by the artifact dispatcher to disambiguate `Architecture::Universal`:
/// when REAPER is universal and RABBIT happens to be running under Rosetta,
/// `Architecture::current()` returns `X64` (the slice the kernel handed
/// the translator), but REAPER launched normally will run as `arm64`. So
/// per-arch plug-ins must be the `arm64` ones, not `x86_64`.
///
/// Always `false` on non-macOS hosts, on Intel Macs (the
/// `sysctl.proc_translated` key only exists on Apple Silicon kernels),
/// and on Apple Silicon Macs running native binaries.
pub fn is_running_under_rosetta() -> bool {
    #[cfg(target_os = "macos")]
    {
        rosetta_via_sysctlbyname()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// macOS-only: read `sysctl.proc_translated` for the calling process
/// via `sysctlbyname` in libSystem (linked automatically by every Rust
/// binary on macOS, so no extra crate dependency is needed).
///
/// Earlier RABBIT releases (≤ 0.1.1) shelled out to `/usr/sbin/sysctl`
/// to read the same key, but `sysctl.proc_translated` is per-process —
/// it reports whether the *querying* process is translated, and the
/// shelled-out `sysctl` binary always runs as the host's native arch
/// (the kernel picks its native slice at exec time, regardless of the
/// parent's translation state). The shell-out therefore always
/// reported `0`, even when the *parent* RABBIT process was running
/// under Rosetta. Calling `sysctlbyname` from inside RABBIT itself
/// fixes that: the kernel resolves the value against the calling
/// thread's translation state, which is what the dispatcher needs.
#[cfg(target_os = "macos")]
fn rosetta_via_sysctlbyname() -> bool {
    use std::os::raw::{c_char, c_int, c_void};

    // SAFETY: `sysctlbyname` is part of the BSD layer of libSystem, ABI-stable
    // on macOS. We pass a NUL-terminated key name, a properly-sized output
    // pointer for the `int` value, no input buffer, and no input length.
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }

    let name = c"sysctl.proc_translated";
    let mut value: c_int = 0;
    let mut size = std::mem::size_of::<c_int>();
    let result = unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut c_int).cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    // Non-zero return means the key wasn't found (Intel Mac kernels
    // don't expose it) or some other failure; either way, treat as
    // "not under Rosetta".
    result == 0 && value == 1
}

#[cfg(test)]
mod tests {
    use super::is_running_under_rosetta;

    #[test]
    fn never_reports_rosetta_on_non_macos_targets() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert!(!is_running_under_rosetta());
    }
}
