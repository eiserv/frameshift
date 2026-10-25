//! The V1 feature catalog of the `nextjs-to-astro-preact` migration path.
//!
//! Every construct that analysis recognizes has a stable feature ID and the
//! assessment statuses that the compatibility matrix in section 17 of
//! MIGRATION_SPEC.md allows for it in V1. The catalog is embedded data
//! without I/O, framework facts, or rules: detectors produce occurrences of
//! these features, and assessment rules choose among the allowed statuses.
//! A feature whose matrix entry allows exactly one status has that status
//! fixed; every other one needs a rule. Constructs that no entry describes
//! are occurrences of [`UNKNOWN`].

/// The migration path that this catalog belongs to.
pub const PATH: &str = "nextjs-to-astro-preact";

/// The generic feature of constructs that analysis cannot match to the
/// catalog or cannot analyze at all.
pub const UNKNOWN: &str = "source.unknown";

/// An assessment status, ordered by severity as in section 3.1 of
/// MIGRATION_REPORTING.md: `unsupported` ranks above `unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// A rule handles the construct deterministically.
    Auto,
    /// A rule transforms it, but correctness needs human review.
    Review,
    /// Not recognized or not determinable.
    Unknown,
    /// Recognized and known not to be migratable in this stage.
    Unsupported,
}

impl Status {
    /// The stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Review => "review",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        }
    }
}

/// What the V1 matrix allows for a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V1 {
    /// The allowed assessment statuses, sorted by severity and distinct.
    Assessed(&'static [Status]),
    /// Reported separately and not migrated in V1; never assessed.
    OutOfScope,
}

impl V1 {
    /// The only allowed status, when the matrix allows exactly one.
    pub const fn fixed(self) -> Option<Status> {
        match self {
            Self::Assessed([status]) => Some(*status),
            Self::Assessed(_) | Self::OutOfScope => None,
        }
    }
}

/// The subsection of the compatibility matrix that lists a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    /// 17.1 Project and configuration.
    Project,
    /// 17.2 Routing.
    Routing,
    /// 17.3 Rendering and data.
    Rendering,
    /// 17.4 Components and React.
    Components,
    /// 17.5 Next.js APIs, styling, assets, metadata.
    Platform,
    /// 17.6 Unrecognized constructs.
    Unrecognized,
}

/// One concrete catalog entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    /// The stable feature ID.
    pub id: &'static str,
    /// A short English name.
    pub name: &'static str,
    /// Where the matrix lists it.
    pub section: Section,
    /// What V1 allows.
    pub v1: V1,
}

/// A family of features that share an ID prefix, listed as `<prefix>.*` in
/// the matrix. Its members are concrete entries of [`FEATURES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Family {
    /// The ID prefix without the trailing `.*`.
    pub prefix: &'static str,
    /// A short English name.
    pub name: &'static str,
    /// Where the matrix lists it.
    pub section: Section,
    /// What the family row allows; members carry their own statuses.
    pub v1: V1,
}

const A: V1 = V1::Assessed(&[Status::Auto]);
const R: V1 = V1::Assessed(&[Status::Review]);
const U: V1 = V1::Assessed(&[Status::Unsupported]);
const UNKNOWN_ONLY: V1 = V1::Assessed(&[Status::Unknown]);
const AR: V1 = V1::Assessed(&[Status::Auto, Status::Review]);
const AU: V1 = V1::Assessed(&[Status::Auto, Status::Unsupported]);
const RU: V1 = V1::Assessed(&[Status::Review, Status::Unsupported]);
const ARU: V1 = V1::Assessed(&[Status::Auto, Status::Review, Status::Unsupported]);

const fn f(id: &'static str, name: &'static str, section: Section, v1: V1) -> Feature {
    Feature {
        id,
        name,
        section,
        v1,
    }
}

use Section::{Components as C, Platform as P, Project as J, Rendering as N, Routing as T};

/// Every concrete feature, sorted by ID bytes for lookup.
pub static FEATURES: &[Feature] = &[
    f("asset.import", "Static asset import", P, A),
    f("asset.json-import", "JSON module import", P, A),
    f("asset.public", "Public directory asset", P, A),
    f("boundary.jsx-props", "JSX props across the boundary", C, R),
    f(
        "boundary.promise-props",
        "Promise props across the boundary",
        C,
        U,
    ),
    f(
        "boundary.props",
        "Serializable props across the boundary",
        C,
        A,
    ),
    f(
        "component.astro-convertible",
        "Astro-convertible server component",
        C,
        A,
    ),
    f("component.client-page", "Client page", C, R),
    f("component.island", "Client component at a boundary", C, A),
    f(
        "component.preact-static",
        "Other server-context component",
        C,
        A,
    ),
    f("css.global", "Global CSS", P, A),
    f("css.in-js", "Runtime CSS-in-JS", P, U),
    f("css.module", "CSS Module", P, A),
    f("css.sass", "Sass or SCSS", P, A),
    f("css.styled-jsx", "styled-jsx", P, U),
    f("css.tailwind", "Tailwind CSS", P, AR),
    f("deploy.config", "Deployment configuration", J, U),
    f("env.reference", "Environment variable reference", J, AR),
    f("nextjs.api.config", "next/config", P, U),
    f(
        "nextjs.api.dynamic",
        "next/dynamic with server rendering",
        C,
        R,
    ),
    f(
        "nextjs.api.dynamic-no-ssr",
        "next/dynamic with ssr: false",
        C,
        A,
    ),
    f("nextjs.api.font", "next/font", P, R),
    f("nextjs.api.image", "next/image", P, AR),
    f("nextjs.api.link", "next/link", P, AR),
    f("nextjs.api.navigation", "next/navigation hooks", P, R),
    f("nextjs.api.not-found", "notFound", P, R),
    f("nextjs.api.pages-router", "Pages Router API", P, U),
    f(
        "nextjs.api.redirect",
        "redirect and permanentRedirect",
        P,
        U,
    ),
    f("nextjs.api.request", "Request-time API", N, U),
    f("nextjs.api.script", "next/script", P, RU),
    f(
        "nextjs.api.search-params-server",
        "Server searchParams",
        N,
        U,
    ),
    f("nextjs.api.server-action", "Server Action", N, U),
    f("nextjs.api.server-only", "Server-only Next.js module", P, U),
    f("nextjs.config.base-path", "basePath", J, AR),
    f("nextjs.config.compress", "compress", J, A),
    f("nextjs.config.env", "env", J, R),
    f("nextjs.config.eslint", "eslint", J, A),
    f("nextjs.config.headers", "headers", J, U),
    f("nextjs.config.i18n", "i18n", J, U),
    f("nextjs.config.images.domains", "images.domains", J, R),
    f(
        "nextjs.config.images.loader",
        "images.loader or loaderFile",
        J,
        U,
    ),
    f(
        "nextjs.config.images.remote-patterns",
        "images.remotePatterns",
        J,
        R,
    ),
    f(
        "nextjs.config.images.unoptimized",
        "images.unoptimized",
        J,
        A,
    ),
    f("nextjs.config.output", "output: 'export'", J, A),
    f(
        "nextjs.config.page-extensions",
        "pageExtensions including mdx",
        J,
        U,
    ),
    f("nextjs.config.powered-by-header", "poweredByHeader", J, A),
    f("nextjs.config.react-strict-mode", "reactStrictMode", J, A),
    f("nextjs.config.redirects", "redirects", J, R),
    f("nextjs.config.rewrites", "rewrites", J, U),
    f("nextjs.config.trailing-slash", "trailingSlash", J, A),
    f(
        "nextjs.config.transpile-packages",
        "transpilePackages",
        J,
        R,
    ),
    f("nextjs.config.turbopack", "turbopack", J, U),
    f("nextjs.config.typescript", "typescript", J, A),
    f(
        "nextjs.config.unknown",
        "Unrecognized or unevaluated configuration",
        J,
        UNKNOWN_ONLY,
    ),
    f("nextjs.config.webpack", "webpack", J, U),
    f("nextjs.fetch", "Build-time fetch", N, AR),
    f("nextjs.file.error", "error or global-error", T, R),
    f("nextjs.file.layout", "Layout", T, AU),
    f("nextjs.file.loading", "loading", T, R),
    f("nextjs.file.not-found", "not-found", T, AR),
    f("nextjs.file.proxy", "proxy or middleware", T, U),
    f("nextjs.file.route-handler", "Route handler", T, U),
    f(
        "nextjs.file.route-handler-static",
        "Static GET route handler",
        T,
        R,
    ),
    f("nextjs.file.template", "template", T, R),
    f("nextjs.guard", "server-only or client-only", N, A),
    f("nextjs.metadata.dynamic", "Non-literal metadata", P, R),
    f("nextjs.metadata.file", "Static metadata file", P, A),
    f("nextjs.metadata.route", "Generated metadata file", P, RU),
    f("nextjs.metadata.static", "Static metadata", P, A),
    f(
        "nextjs.metadata.unsupported-field",
        "Unsupported metadata field",
        P,
        U,
    ),
    f(
        "nextjs.navigation.client",
        "Client-side navigation semantics",
        T,
        R,
    ),
    f("nextjs.project.app-router", "App Router project", J, A),
    f("nextjs.project.pages-router", "Pages Router project", J, U),
    f("nextjs.route.catch-all", "Catch-all segment", T, AR),
    f("nextjs.route.dynamic", "Dynamic segment", T, AR),
    f(
        "nextjs.route.dynamic-unenumerable",
        "Dynamic segment without generateStaticParams",
        T,
        U,
    ),
    f("nextjs.route.group", "Route group", T, A),
    f("nextjs.route.intercepting", "Intercepting route", T, U),
    f(
        "nextjs.route.optional-catch-all",
        "Optional catch-all segment",
        T,
        R,
    ),
    f("nextjs.route.page", "Static page", T, A),
    f("nextjs.route.parallel", "Parallel route", T, U),
    f("nextjs.route.private", "Private folder", T, A),
    f("nextjs.route.segment", "Nested segment", T, A),
    f("nextjs.segment.dynamic", "force-dynamic", N, U),
    f("nextjs.segment.revalidate", "Segment revalidate", N, R),
    f("nextjs.segment.runtime", "Segment runtime", N, A),
    f("npm.lockfile", "Lockfile", J, A),
    f("npm.lockfile.bun-binary", "Bun binary lockfile", J, R),
    f("npm.manifest", "Package manifest and scripts", J, AR),
    f("npm.workspace", "Monorepo workspace package", J, R),
    f("react.api.children", "Children", C, A),
    f("react.api.clone-element", "cloneElement", C, A),
    f("react.api.component", "Class component", C, A),
    f("react.api.create-context", "createContext", C, A),
    f("react.api.create-element", "createElement", C, A),
    f("react.api.create-portal", "createPortal", C, A),
    f("react.api.forward-ref", "forwardRef", C, A),
    f("react.api.fragment", "Fragment", C, A),
    f("react.api.is-valid-element", "isValidElement", C, A),
    f("react.api.lazy", "lazy", C, A),
    f("react.api.memo", "memo", C, A),
    f("react.api.start-transition", "startTransition", C, AR),
    f("react.api.strict-mode", "StrictMode", C, A),
    f("react.api.suspense", "Client-side Suspense", C, A),
    f("react.cache", "React cache or unstable_cache", N, R),
    f("react.context.cross-island", "Context across islands", C, U),
    f("react.context.local", "Context within one island", C, A),
    f(
        "react.head-hoisting",
        "Head elements rendered in components",
        C,
        R,
    ),
    f("react.hook.react19", "React 19-only hook", C, U),
    f("react.hook.use-callback", "useCallback", C, A),
    f("react.hook.use-context", "useContext", C, A),
    f("react.hook.use-deferred-value", "useDeferredValue", C, AR),
    f("react.hook.use-effect", "useEffect", C, A),
    f("react.hook.use-id", "useId", C, R),
    f(
        "react.hook.use-imperative-handle",
        "useImperativeHandle",
        C,
        A,
    ),
    f(
        "react.hook.use-insertion-effect",
        "useInsertionEffect",
        C,
        AR,
    ),
    f("react.hook.use-layout-effect", "useLayoutEffect", C, A),
    f("react.hook.use-memo", "useMemo", C, A),
    f("react.hook.use-reducer", "useReducer", C, A),
    f("react.hook.use-ref", "useRef", C, A),
    f("react.hook.use-state", "useState", C, A),
    f(
        "react.hook.use-sync-external-store",
        "useSyncExternalStore",
        C,
        AR,
    ),
    f("react.hook.use-transition", "useTransition", C, AR),
    f("react.server-component", "Server component", N, AU),
    f("react.suspense-server", "Server Suspense", N, A),
    f(
        UNKNOWN,
        "Unrecognized or unanalyzable construct",
        Section::Unrecognized,
        UNKNOWN_ONLY,
    ),
    f("test.setup", "Test setup", J, V1::OutOfScope),
    f("ts.config", "TypeScript or JavaScript configuration", J, A),
];

/// Every family, sorted by prefix.
pub static FAMILIES: &[Family] = &[
    Family {
        prefix: "nextjs.config",
        name: "Static configuration keys",
        section: Section::Project,
        v1: ARU,
    },
    Family {
        prefix: "react.api",
        name: "Compat-covered React APIs",
        section: Section::Components,
        v1: A,
    },
    Family {
        prefix: "react.hook",
        name: "Compat-covered React hooks",
        section: Section::Components,
        v1: A,
    },
];

/// Looks up a concrete feature by its exact ID.
pub fn feature(id: &str) -> Option<&'static Feature> {
    FEATURES
        .binary_search_by(|feature| feature.id.cmp(id))
        .ok()
        .map(|index| &FEATURES[index])
}

/// The family whose prefix the ID extends, if any.
pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|family| {
        id.strip_prefix(family.prefix)
            .is_some_and(|rest| rest.starts_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_sorted_distinct_and_consistent() {
        assert!(FEATURES.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert!(
            FAMILIES
                .windows(2)
                .all(|pair| pair[0].prefix < pair[1].prefix)
        );
        for feature in FEATURES
            .iter()
            .map(|f| f.v1)
            .chain(FAMILIES.iter().map(|f| f.v1))
        {
            if let V1::Assessed(statuses) = feature {
                assert!(!statuses.is_empty());
                assert!(statuses.windows(2).all(|pair| pair[0] < pair[1]));
            }
        }
        for entry in FEATURES {
            assert_eq!(feature(entry.id), Some(entry));
        }
        assert_eq!(feature("nextjs.config"), None);
        assert_eq!(
            feature(UNKNOWN).and_then(|f| f.v1.fixed()),
            Some(Status::Unknown)
        );
        assert_eq!(feature("test.setup").unwrap().v1.fixed(), None);
        assert_eq!(feature("nextjs.route.dynamic").unwrap().v1.fixed(), None);
        assert_eq!(family("react.hook.use-id").unwrap().prefix, "react.hook");
        assert_eq!(family("react.hooks.x"), None);
        assert_eq!(family("nextjs.config"), None);
    }

    #[test]
    fn severity_order_matches_the_status_model() {
        assert!(Status::Auto < Status::Review);
        assert!(Status::Review < Status::Unknown);
        assert!(Status::Unknown < Status::Unsupported);
    }
}
