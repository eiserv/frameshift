//! The Next.js source adapter (`nextjs`): whether the selected package is a
//! Next.js project, which Next.js version it uses, which configuration file
//! Next.js loads, and which routers it has (`D-074`).
//!
//! Detection combines evidence, as section 7 of ARCHITECTURE.md requires:
//! a `next` dependency and at least one structural signal, a
//! configuration file or route files. It reads no file content; the
//! inventory, the manifest, and the root lockfile suffice. Framework
//! knowledge lives here, not in the package or input crates.

mod detect;

#[cfg(test)]
mod tests;

pub use detect::{
    ConfigFile, ConfigLoad, DetectError, Detection, NextDependency, Router, RouterDirectory,
    RouterState, VALIDATED_MAJORS, Verdict, VersionEvidence, detect,
};

/// The source adapter identifier.
pub const ADAPTER_ID: &str = "nextjs";
