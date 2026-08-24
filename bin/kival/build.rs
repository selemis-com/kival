//! Build script for Kival.

use std::{
    env,
    error::Error,
    path::MAIN_SEPARATOR,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> Result<(), Box<dyn Error>> {
    // Re-run if git state changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-env-changed=KIVAL_RELEASE_VERSION");

    let sha = git(&["rev-parse", "HEAD"])?;
    let sha_short = &sha[0..7];

    let is_dirty = !git(&["status", "--porcelain"])?.is_empty();

    // A release build must be checked out at an exact tag. Unlike
    // `git describe --always --tags`, this also classifies a raw abbreviated SHA
    // from a shallow or tag-less checkout as a development build.
    let is_tagged = git_succeeds(&["describe", "--exact-match", "--tags", "HEAD"])?;
    let version_suffix = if is_dirty || !is_tagged { "-dev" } else { "" };
    println!("cargo:rustc-env=KIVAL_VERSION_SUFFIX={version_suffix}");

    // Set the build profile
    let out_dir = env::var("OUT_DIR").unwrap();
    let profile = out_dir.rsplit(MAIN_SEPARATOR).nth(3).unwrap();
    println!("cargo:rustc-env=KIVAL_BUILD_PROFILE={profile}");

    // Build timestamp (RFC3339, UTC, nanosecond precision).
    // Honors `SOURCE_DATE_EPOCH` for reproducible builds (matches vergen).
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let build_timestamp = build_timestamp();

    // Comma-joined list of enabled cargo features (lower-cased).
    let features = cargo_features();

    // Set formatted version strings. Release builds may override the displayed
    // version with the exact stable or release-candidate tag while retaining
    // Cargo's stable package version for dependency resolution.
    let pkg_version = env!("CARGO_PKG_VERSION");
    let release_version = release_version(pkg_version)?;
    println!("cargo:rustc-env=KIVAL_RELEASE_VERSION={release_version}");

    // The short version information for Kival.
    // - The latest version from Cargo.toml
    // - The short SHA of the latest commit.
    // Example: 0.1.0 (defa64b2)
    println!("cargo:rustc-env=KIVAL_SHORT_VERSION={release_version}{version_suffix} ({sha_short})");

    // The long version information for Kival.
    //
    // - The latest version from Cargo.toml + version suffix (if any)
    // - The full SHA of the latest commit
    // - The build datetime
    // - The build features
    // - The build profile
    //
    // Example:
    //
    // ```text
    // Version: 0.1.0-dev
    // Commit SHA: 85032e90629ec6637181b7a488693764aabd9d4c
    // Build Timestamp: 2026-05-10T18:06:49.583231000Z
    // Build Features: default,min_trace_logs
    // Build Profile: debug
    // ```
    println!("cargo:rustc-env=KIVAL_LONG_VERSION_0=Version: {release_version}{version_suffix}");
    println!("cargo:rustc-env=KIVAL_LONG_VERSION_1=Commit SHA: {sha}");
    println!("cargo:rustc-env=KIVAL_LONG_VERSION_2=Build Timestamp: {build_timestamp}");
    println!("cargo:rustc-env=KIVAL_LONG_VERSION_3=Build Features: {features}");
    println!("cargo:rustc-env=KIVAL_LONG_VERSION_4=Build Profile: {profile}");

    Ok(())
}

/// Resolve the user-visible binary version.
///
/// Normal builds use the Cargo package version. The release workflow may set
/// `KIVAL_RELEASE_VERSION` to the same stable version or an `-rcN` version with
/// the same stable base. Rejecting every other override prevents a malformed
/// workflow environment from producing mislabeled binaries.
fn release_version(pkg_version: &str) -> Result<String, Box<dyn Error>> {
    let Ok(version) = env::var("KIVAL_RELEASE_VERSION") else {
        return Ok(pkg_version.to_owned());
    };

    if version == pkg_version {
        return Ok(version);
    }

    let rc_prefix = format!("{pkg_version}-rc");
    let Some(rc_number) = version.strip_prefix(&rc_prefix) else {
        return Err(format!(
            "KIVAL_RELEASE_VERSION must be {pkg_version} or {pkg_version}-rcN, got {version}"
        )
        .into());
    };

    if rc_number.is_empty()
        || rc_number.starts_with('0')
        || !rc_number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!(
            "KIVAL_RELEASE_VERSION must be {pkg_version} or {pkg_version}-rcN, got {version}"
        )
        .into());
    }

    Ok(version)
}

/// Run `git` with the given args and return trimmed stdout.
fn git_succeeds(args: &[&str]) -> Result<bool, Box<dyn Error>> {
    Ok(Command::new("git").args(args).output()?.status.success())
}

/// Runs Git with the provided arguments and returns trimmed standard output.
fn git(args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Collect cargo features from `CARGO_FEATURE_*` env vars.
///
/// Cargo sets one such variable per enabled feature, with the name
/// uppercased and dashes replaced by underscores. We lowercase them and
/// join with commas. Order matches `env::vars()` iteration order, which
/// is what vergen does (i.e. unsorted).
fn cargo_features() -> String {
    let features: Vec<String> = env::vars()
        .filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(str::to_lowercase))
        .collect();
    features.join(",")
}

/// Current time as RFC3339 UTC with nanosecond precision, e.g.
/// `2026-05-10T18:06:49.583231000Z`.
///
/// Honors `SOURCE_DATE_EPOCH` (Unix seconds, no sub-second component) for
/// reproducible builds, matching vergen's behavior.
fn build_timestamp() -> String {
    let (secs, nanos) =
        env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse::<u64>().ok()).map_or_else(
            || {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock before UNIX epoch");
                (now.as_secs(), now.subsec_nanos())
            },
            |s| (s, 0_u32),
        );

    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hour = tod / 3600;
    let min = (tod % 3600) / 60;
    let sec = tod % 60;

    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{nanos:09}Z")
}

/// Convert days since the Unix epoch (1970-01-01) to `(year, month, day)`
/// in the proleptic Gregorian calendar. Based on Howard Hinnant's
/// `civil_from_days` algorithm: <https://howardhinnant.github.io/date_algorithms.html>.
const fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
