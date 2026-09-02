//! Build script for Kival's `PostgreSQL` state machine.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=migrations");
}
