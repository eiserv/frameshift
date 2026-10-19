//! Dependency specifiers: what a `package.json` dependency value requests
//! (`D-069`).

use crate::range::VersionRange;

/// The kind of a dependency specifier, classified in a fixed order.
///
/// The order follows npm's own classification where npm defines one:
/// aliases, workspace and catalog protocols, local paths, Git sources, URLs
/// and other protocols, then version ranges and tags. Ranges use the
/// default node-semver grammar; a value that only npm's loose grammar
/// accepts is [`DependencySpec::Invalid`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencySpec {
    /// A version range, including an exact version, `*`, and the empty string.
    Range(VersionRange),
    /// A distribution tag such as `latest` or `canary`: characters that
    /// `encodeURIComponent` keeps, and no digit.
    Tag(Box<str>),
    /// `npm:<name>[@<spec>]`: another registry package under this name. The
    /// target is a range or a tag; a missing one means `latest`.
    Alias {
        /// The package actually installed.
        name: Box<str>,
        /// The requested version of that package.
        target: Box<DependencySpec>,
    },
    /// `workspace:<text>` of pnpm, Yarn, and Bun, with the text after the colon.
    Workspace(Box<str>),
    /// `catalog:<name>` of pnpm and Bun, with the catalog name, empty for
    /// the default catalog.
    Catalog(Box<str>),
    /// `file:`, `link:`, or `portal:`, or a relative, home, absolute, or
    /// drive path, or a value with a slash or a tarball extension.
    Local,
    /// A Git source: `git+` or `git:` URLs, hosted shorthands such as
    /// `github:`, `owner/repo`, and `user@host:path`.
    Git,
    /// An `http:` or `https:` URL.
    Url,
    /// Another protocol, such as `patch:` or `jsr:`.
    OtherProtocol,
    /// None of the above, for example a range only npm's loose grammar
    /// accepts or a tag with characters npm rejects.
    Invalid,
}

impl DependencySpec {
    /// Classifies a specifier text.
    pub fn parse(text: &str) -> Self {
        let lower = |prefix: &str| {
            text.get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        };
        if lower("npm:") {
            return alias(&text[4..]);
        }
        if lower("workspace:") {
            return Self::Workspace(text[10..].into());
        }
        if lower("catalog:") {
            return Self::Catalog(text[8..].into());
        }
        if lower("file:") || lower("link:") || lower("portal:") || is_path(text) {
            return Self::Local;
        }
        if is_git(text) {
            return Self::Git;
        }
        if let Some(protocol) = protocol(text) {
            return if protocol.eq_ignore_ascii_case("http")
                || protocol.eq_ignore_ascii_case("https")
            {
                Self::Url
            } else {
                Self::OtherProtocol
            };
        }
        if text.contains('/') || is_tarball(text) {
            return Self::Local;
        }
        registry(text)
    }

    /// Returns the range of a range or of an alias to a range.
    pub fn range(&self) -> Option<&VersionRange> {
        match self {
            Self::Range(range) => Some(range),
            Self::Alias { target, .. } => target.range(),
            Self::Tag(_)
            | Self::Workspace(_)
            | Self::Catalog(_)
            | Self::Local
            | Self::Git
            | Self::Url
            | Self::OtherProtocol
            | Self::Invalid => None,
        }
    }
}

/// A range or a tag, as npm resolves registry specifiers.
fn registry(text: &str) -> DependencySpec {
    if let Ok(range) = VersionRange::parse(text) {
        return DependencySpec::Range(range);
    }
    let trimmed = text.trim();
    let tag = !trimmed.is_empty()
        && trimmed.bytes().all(|byte| {
            byte.is_ascii_alphabetic()
                || matches!(
                    byte,
                    b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
                )
        });
    if tag {
        DependencySpec::Tag(trimmed.into())
    } else {
        DependencySpec::Invalid
    }
}

fn alias(rest: &str) -> DependencySpec {
    let split = rest
        .get(1..)
        .and_then(|tail| tail.find('@'))
        .map(|index| index + 1);
    let (name, target) = match split {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (rest, ""),
    };
    if !is_package_name(name) {
        return DependencySpec::Invalid;
    }
    let target = if target.is_empty() {
        DependencySpec::Tag("latest".into())
    } else {
        registry(target)
    };
    match target {
        DependencySpec::Range(_) | DependencySpec::Tag(_) => DependencySpec::Alias {
            name: name.into(),
            target: Box::new(target),
        },
        DependencySpec::Alias { .. }
        | DependencySpec::Workspace(_)
        | DependencySpec::Catalog(_)
        | DependencySpec::Local
        | DependencySpec::Git
        | DependencySpec::Url
        | DependencySpec::OtherProtocol
        | DependencySpec::Invalid => DependencySpec::Invalid,
    }
}

/// A package name as npm accepts it for new packages: lowercase URL-safe
/// characters, at most 214 bytes, optionally scoped, not starting with a
/// dot or underscore.
pub(crate) fn is_package_name(name: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && !part.starts_with(['.', '_'])
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(
                        byte,
                        b'-' | b'.' | b'_' | b'~' | b'!' | b'*' | b'\'' | b'(' | b')'
                    )
            })
    };
    if name.is_empty() || name.len() > 214 {
        return false;
    }
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| part(scope) && part(rest)),
        None => part(name),
    }
}

/// `.`, `~/`, `/`, `\`, or a drive letter and colon at the start.
fn is_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    matches!(bytes.first(), Some(b'.' | b'/' | b'\\'))
        || text.starts_with("~/")
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn is_tarball(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.ends_with(".tgz") || lower.ends_with(".tar.gz") || lower.ends_with(".tar")
}

/// The protocol of `[git+]<letters>:`.
fn protocol(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("git+").unwrap_or(text);
    let end = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    (end > 0 && rest.as_bytes().get(end) == Some(&b':')).then(|| &rest[..end])
}

fn is_git(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("git+")
        || lower.starts_with("git:")
        || ["github:", "gitlab:", "bitbucket:", "gist:"]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }
    // `user@host.tld:path`, which npm reads as `git+ssh://`.
    if let Some((user, rest)) = text.split_once('@')
        && !user.is_empty()
        && let Some((host, path)) = rest.split_once(':')
        && host.contains('.')
        && !path.is_empty()
    {
        return true;
    }
    // `owner/repo` with an optional `#committish`, unless it names a tarball.
    let (repository, _) = text.split_once('#').unwrap_or((text, ""));
    if is_tarball(repository) {
        return false;
    }
    repository.split_once('/').is_some_and(|(owner, name)| {
        let segment = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        };
        segment(owner) && segment(name) && !owner.starts_with('.')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(text: &str) -> &'static str {
        match DependencySpec::parse(text) {
            DependencySpec::Range(_) => "range",
            DependencySpec::Tag(_) => "tag",
            DependencySpec::Alias { .. } => "alias",
            DependencySpec::Workspace(_) => "workspace",
            DependencySpec::Catalog(_) => "catalog",
            DependencySpec::Local => "local",
            DependencySpec::Git => "git",
            DependencySpec::Url => "url",
            DependencySpec::OtherProtocol => "protocol",
            DependencySpec::Invalid => "invalid",
        }
    }

    #[test]
    fn classifies_specifiers() {
        for (text, expected) in [
            ("^16.2.1", "range"),
            ("", "range"),
            ("*", "range"),
            ("1.2.3 - 2", "range"),
            ("latest", "tag"),
            ("canary", "tag"),
            ("next-15", "invalid"),
            ("1.2.3beta", "invalid"),
            ("has space", "invalid"),
            ("npm:other@^1.0.0", "alias"),
            ("NPM:@scope/other@latest", "alias"),
            ("npm:other", "alias"),
            ("npm:Other@1", "invalid"),
            ("npm:other@file:../x", "invalid"),
            ("workspace:*", "workspace"),
            ("workspace:^", "workspace"),
            ("catalog:", "catalog"),
            ("catalog:react19", "catalog"),
            ("file:../local", "local"),
            ("link:../local", "local"),
            ("./local", "local"),
            ("~/local", "local"),
            ("/abs", "local"),
            ("C:\\dir", "local"),
            ("vendor/pkg.tgz", "local"),
            ("pkg.tar.gz", "local"),
            ("a/b/c", "local"),
            ("git+https://example.test/r.git", "git"),
            ("git://example.test/r.git", "git"),
            ("github:owner/repo#v1", "git"),
            ("owner/repo", "git"),
            ("owner/repo#semver:^1", "git"),
            ("git@example.test:owner/repo.git", "git"),
            ("https://example.test/p.tgz", "url"),
            ("http://example.test/p", "url"),
            ("patch:pkg@1#./p.patch", "protocol"),
            ("jsr:@std/path", "protocol"),
        ] {
            assert_eq!(kind(text), expected, "{text:?}");
        }
    }

    #[test]
    fn resolves_alias_targets() {
        let DependencySpec::Alias { name, target } = DependencySpec::parse("npm:@scope/other@~2.1")
        else {
            panic!("alias expected");
        };
        assert_eq!(&*name, "@scope/other");
        assert_eq!(
            target.range().map(ToString::to_string).as_deref(),
            Some(">=2.1.0 <2.2.0-0")
        );
        assert_eq!(
            DependencySpec::parse("npm:other"),
            DependencySpec::Alias {
                name: "other".into(),
                target: Box::new(DependencySpec::Tag("latest".into())),
            }
        );
    }

    #[test]
    fn validates_package_names() {
        for valid in ["next", "@scope/name", "a.b-c_d", "x~y"] {
            assert!(is_package_name(valid), "{valid}");
        }
        for invalid in [
            "",
            ".hidden",
            "_private",
            "Upper",
            "@scope",
            "@/x",
            "a b",
            &"a".repeat(215),
        ] {
            assert!(!is_package_name(invalid), "{invalid}");
        }
    }
}
