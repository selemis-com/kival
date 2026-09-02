//! Version endpoint information for Kival.

use crate::gauge;

/// Contains version information for Kival.
#[derive(Debug, Clone, Copy)]
pub struct VersionInfo {
    /// The version of Kival.
    pub version: &'static str,
    /// The build timestamp of Kival.
    pub build_timestamp: &'static str,
    /// The cargo features enabled for the build.
    pub cargo_features: &'static str,
    /// The Git SHA of the build.
    pub git_sha: &'static str,
    /// The target triple for the build.
    pub target_triple: &'static str,
    /// The build profile (e.g., debug or release).
    pub build_profile: &'static str,
}

impl VersionInfo {
    /// This exposes Kival's version information over prometheus.
    pub fn register_version_metrics(&self) {
        let labels: [(&str, &str); 6] = [
            ("version", self.version),
            ("build_timestamp", self.build_timestamp),
            ("cargo_features", self.cargo_features),
            ("git_sha", self.git_sha),
            ("target_triple", self.target_triple),
            ("build_profile", self.build_profile),
        ];

        let gauge = gauge!("info", &labels);
        gauge.set(1.0);
    }
}
