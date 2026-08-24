//! Version strings for the Kival binary.

/// Short version string for Kival, set by `build.rs`.
pub const SHORT_VERSION: &str = env!("KIVAL_SHORT_VERSION");

/// The exact stable or release-candidate version represented by this binary.
pub const KIVAL_RELEASE_VERSION: &str = env!("KIVAL_RELEASE_VERSION");

/// The full SHA of the latest commit.
pub const KIVAL_GIT_SHA_LONG: &str = env!("KIVAL_GIT_SHA");

/// The 8 character short SHA of the latest commit.
pub const KIVAL_GIT_SHA: &str = env!("KIVAL_GIT_SHA_SHORT");

/// The build timestamp.
pub const KIVAL_BUILD_TIMESTAMP: &str = env!("KIVAL_BUILD_TIMESTAMP");

/// The target triple.
pub const KIVAL_CARGO_TARGET_TRIPLE: &str = env!("KIVAL_CARGO_TARGET_TRIPLE");

/// The build features.
pub const KIVAL_CARGO_FEATURES: &str = env!("KIVAL_CARGO_FEATURES");

/// The build profile.
pub const KIVAL_BUILD_PROFILE: &str = env!("KIVAL_BUILD_PROFILE");

/// Long version string for Kival, set by `build.rs`.
pub const LONG_VERSION: &str = concat!(
    env!("KIVAL_LONG_VERSION_0"),
    "\n",
    env!("KIVAL_LONG_VERSION_1"),
    "\n",
    env!("KIVAL_LONG_VERSION_2"),
    "\n",
    env!("KIVAL_LONG_VERSION_3"),
    "\n",
    env!("KIVAL_LONG_VERSION_4"),
);
