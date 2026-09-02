//! Version strings for the Kival binary.

/// Short version string for Kival, set by `build.rs`.
pub const SHORT_VERSION: &str = env!("KIVAL_SHORT_VERSION");

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
