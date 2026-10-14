//! Build identity and the implemented migration catalogs, without runtime I/O.

/// Versions embedded in this engine build, independent of the host at runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionInfo {
    /// Cargo package version of the engine, inherited from the workspace.
    pub engine_version: &'static str,
    /// Shipped migration ruleset identifiers with their versions.
    pub rulesets: &'static [&'static str],
    /// Shipped, versioned package and framework knowledge bases.
    pub knowledge_bases: &'static [&'static str],
    /// Shipped migration target profile identifiers.
    pub target_profiles: &'static [&'static str],
    /// Explicitly injected build commit; unknown in ordinary local builds.
    pub build_commit: Option<&'static str>,
    /// Actual compiler identity captured by the engine's Cargo build script.
    pub toolchain: &'static str,
}

/// Returns build identity and empty catalogs until migration data is shipped.
///
/// The development-only E2 fixtures do not constitute a product ruleset,
/// knowledge base, or target profile. This function reads no environment,
/// filesystem, clock, or process state.
pub const fn info() -> VersionInfo {
    VersionInfo {
        engine_version: env!("CARGO_PKG_VERSION"),
        rulesets: &[],
        knowledge_bases: &[],
        target_profiles: &[],
        build_commit: option_env!("FRAMESHIFT_BUILD_COMMIT"),
        toolchain: env!("FRAMESHIFT_RUSTC_VERSION"),
    }
}
