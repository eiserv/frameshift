//! How each modeled package manager release turns workspace patterns into
//! member directories (`D-073`).
//!
//! The rules were read from the sources of the reference releases and
//! checked against them on generated fictional trees (IMPLEMENTATION.md
//! section 30). Discovery's inventory stands in for the filesystem: a
//! pattern that could reach an entry that discovery did not traverse makes
//! the result incomplete instead of guessing what lies there.

use std::collections::{BTreeMap, BTreeSet};

use frameshift_core::{SourcePath, TextLocation};

use super::matcher::{Encoding, Exhausted, Matcher, Rules, Work, units};
use super::pattern::{ParseBounds, Parsed, Pattern, PatternRejection, Segment, parse};
use crate::load::LoadError;

/// A package manager release whose workspace pattern semantics are modeled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkspaceSemantics {
    /// npm 11, as `@npmcli/map-workspaces` 5.0.3 with glob 13 and minimatch
    /// 10 reads `workspaces` in npm 11.11.0.
    Npm,
    /// Yarn 1, as glob 7 reads `workspaces` in Yarn 1.22.22.
    YarnClassic,
    /// Yarn 2 and 3, as globby reads `workspaces` in Yarn 2.4.3 and 3.8.7.
    YarnBerryV2,
    /// Yarn 4, as fast-glob reads `workspaces` in Yarn 4.18.1.
    YarnBerryV4,
    /// pnpm 10, as tinyglobby reads `pnpm-workspace.yaml` in pnpm 10.34.7.
    PnpmV10,
    /// pnpm 11, as tinyglobby reads `pnpm-workspace.yaml` in pnpm 11.28.6,
    /// which matches negations against dot names too.
    PnpmV11,
    /// pnpm 12, as wax reads `pnpm-workspace.yaml` in pnpm 12.14.0.
    PnpmV12,
    /// Bun 1.4, as Bun 1.4.0 and 1.4.3 read `workspaces`.
    Bun,
}

impl WorkspaceSemantics {
    /// Every modeled release, in a fixed order.
    pub const ALL: [Self; 8] = [
        Self::Npm,
        Self::YarnClassic,
        Self::YarnBerryV2,
        Self::YarnBerryV4,
        Self::PnpmV10,
        Self::PnpmV11,
        Self::PnpmV12,
        Self::Bun,
    ];

    /// Returns the stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Npm => "npm-11",
            Self::YarnClassic => "yarn-1",
            Self::YarnBerryV2 => "yarn-2",
            Self::YarnBerryV4 => "yarn-4",
            Self::PnpmV10 => "pnpm-10",
            Self::PnpmV11 => "pnpm-11",
            Self::PnpmV12 => "pnpm-12",
            Self::Bun => "bun-1.4",
        }
    }

    /// Returns the releases that the name describes, for finding prose.
    pub const fn releases(self) -> &'static str {
        match self {
            Self::Npm => "npm 11",
            Self::YarnClassic => "Yarn 1",
            Self::YarnBerryV2 => "Yarn 2 and 3",
            Self::YarnBerryV4 => "Yarn 4",
            Self::PnpmV10 => "pnpm 10",
            Self::PnpmV11 => "pnpm 11",
            Self::PnpmV12 => "pnpm 12",
            Self::Bun => "Bun 1.4",
        }
    }

    /// Whether the manager reads `pnpm-workspace.yaml` rather than the
    /// `workspaces` field of `package.json`.
    pub const fn reads_pnpm_workspace(self) -> bool {
        matches!(self, Self::PnpmV10 | Self::PnpmV11 | Self::PnpmV12)
    }

    const fn negation(self) -> bool {
        !matches!(self, Self::YarnClassic)
    }

    fn rules(self, fold_case: bool) -> Rules {
        let javascript = matches!(
            self,
            Self::Npm
                | Self::YarnClassic
                | Self::YarnBerryV2
                | Self::YarnBerryV4
                | Self::PnpmV10
                | Self::PnpmV11
        );
        Rules {
            encoding: if javascript {
                Encoding::Utf16
            } else {
                Encoding::Scalar
            },
            dot: false,
            class_matches_leading_dot: matches!(
                self,
                Self::YarnBerryV2 | Self::YarnBerryV4 | Self::PnpmV10 | Self::PnpmV11
            ),
            trailing_globstar_nonempty: false,
            fold_case,
        }
    }

    /// Whether the manager's built-in ignores exclude a directory: npm,
    /// Yarn 2 and later, and pnpm 12 by any segment; pnpm 10 and 11 only
    /// when no segment before it starts with `.`, because the leading `**`
    /// of `**/node_modules/**` does not cross dot directories.
    fn prunes(self, relative: &[Box<str>]) -> bool {
        let pruned = self.pruned_names();
        match self {
            Self::PnpmV10 | Self::PnpmV11 => relative.iter().enumerate().any(|(index, segment)| {
                pruned.contains(&&**segment)
                    && relative[..index]
                        .iter()
                        .all(|segment| !segment.starts_with('.'))
            }),
            Self::Npm
            | Self::YarnClassic
            | Self::YarnBerryV2
            | Self::YarnBerryV4
            | Self::PnpmV12
            | Self::Bun => relative.iter().any(|segment| pruned.contains(&&**segment)),
        }
    }

    /// Directory names that the manager never descends into or returns
    /// through a glob.
    fn pruned_names(self) -> &'static [&'static str] {
        match self {
            Self::Npm => &["node_modules"],
            Self::YarnClassic => &[],
            Self::YarnBerryV2 | Self::YarnBerryV4 => &["node_modules", ".git", ".yarn"],
            Self::PnpmV10 | Self::PnpmV11 | Self::PnpmV12 => &["node_modules", "bower_components"],
            Self::Bun => &["node_modules", ".git", "CMakeFiles"],
        }
    }
}

/// A directory of the tree, with its segments.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub path: SourcePath,
    pub segments: Vec<Box<str>>,
}

/// Discovery's view of the directories below the root.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tree {
    /// Traversed real directories, sorted by path, without the root.
    pub dirs: Vec<Node>,
    /// Entries that a package manager could descend into but discovery did
    /// not traverse: ignored directories, links, reparse points, and
    /// directories that were not read.
    pub hidden: Vec<Node>,
}

/// One pattern with the location that reports name.
#[derive(Clone, Debug)]
pub(crate) struct PatternInput {
    pub text: Box<str>,
    pub location: TextLocation,
}

/// The patterns of one declaration.
#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub patterns: Vec<PatternInput>,
    /// Whether every element of the declaration was a string; a dropped
    /// element leaves the patterns unknown.
    pub complete: bool,
}

/// How a manifest file of a directory appears to discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presence {
    /// No entry with this name.
    Absent,
    /// An admitted file.
    Admitted,
    /// An entry that is not an admitted file, such as an ignored or
    /// oversized file.
    Hidden,
}

/// The facts of a `package.json` that membership depends on.
#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    pub name: Option<Box<str>>,
    pub version: Option<Box<str>>,
    pub private: bool,
    pub workspaces: Option<Declaration>,
}

/// The outcome of reading a `package.json`.
#[derive(Clone, Debug)]
pub(crate) enum Read {
    /// Not analyzed; a finding says why.
    NotAnalyzed,
    /// Analyzed.
    Analyzed(Facts),
}

/// Access to the files of the snapshot.
pub(crate) trait Files {
    /// How the file `name` in `directory` appears.
    fn presence(&self, directory: &SourcePath, name: &str) -> Presence;
    /// Reads the `package.json` of `directory` once, reporting problems.
    fn manifest(&mut self, directory: &SourcePath) -> Result<Read, LoadError>;
}

/// Why a directory's membership is not known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Undetermined {
    /// Its manifest exists but is not an admitted file.
    HiddenManifest,
    /// It has only a manifest format that Frameshift does not read:
    /// `yarn.json` for Yarn 1, `package.yaml` or `package.json5` for pnpm.
    UnreadManifest,
    /// Its `package.json` is not analyzed.
    InvalidManifest,
    /// Its own workspace declaration cannot be evaluated (Yarn 2 and
    /// later resolve nested workspaces).
    NestedUnknown,
}

/// What makes the package manager reject the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FailureKind {
    /// Yarn 1 requires `"private": true` in a workspace root.
    RootNotPrivate,
    /// Two members have the same name.
    DuplicateName,
    /// Bun: a listed directory has no `package.json`.
    MissingListed,
    /// Bun: a member has no `name`.
    MissingName,
    /// A member's `package.json` is not valid.
    InvalidManifest,
}

/// One reason why the package manager would reject the workspace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Failure {
    pub kind: FailureKind,
    /// The directory concerned, `None` for the root.
    pub directory: Option<SourcePath>,
}

/// The members that one release selects.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outcome {
    /// Patterns that are not evaluated.
    pub rejected: Vec<(TextLocation, PatternRejection)>,
    /// Whether a declaration lost elements to type errors.
    pub incomplete_declaration: bool,
    /// Member directories.
    pub members: BTreeSet<SourcePath>,
    /// Directories whose membership is unknown.
    pub undetermined: BTreeMap<SourcePath, Undetermined>,
    /// Entries that discovery did not traverse and a pattern could reach.
    pub hidden: BTreeSet<SourcePath>,
    /// Reasons why the release would reject the workspace.
    pub failures: BTreeSet<Failure>,
    /// Whether `workspace.max-match-work` was reached.
    pub exhausted: bool,
    /// Yarn 1: directories that patterns with and without `./` select.
    pub twice: BTreeSet<SourcePath>,
}

impl Outcome {
    /// Whether the members are known exactly.
    pub(crate) fn is_exact(&self) -> bool {
        self.rejected.is_empty()
            && !self.incomplete_declaration
            && self.undetermined.is_empty()
            && self.hidden.is_empty()
            && !self.exhausted
    }
}

/// Bounds of one evaluation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub parse: ParseBounds,
    pub max_match_work: u64,
}

struct Prepared {
    pattern: Pattern,
    /// Where the pattern is declared.
    location: TextLocation,
    matcher: Matcher,
    /// Yarn 2 and later: how fast-glob reads the pattern, as one form per
    /// brace expansion (Yarn 4) or the unexpanded pattern (Yarn 2 and 3).
    forms: Vec<Form>,
    /// Bun: the walk of a negated pattern, which inverts its first segment.
    negated_walk: Option<Matcher>,
}

/// One pattern as fast-glob reads it.
struct Form {
    /// The literal path of a static pattern, which fast-glob looks up
    /// instead of walking.
    literal: Option<Vec<String>>,
    /// The literal segments of the base directory that a dynamic pattern
    /// walks below.
    base: Vec<String>,
    /// Matches an entry: a positive selects it, a negative excludes it.
    entry: Matcher,
    /// Negatives: whether the pattern stops fast-glob from reading the
    /// directories it matches, and the matcher it uses for that.
    depth: Option<Matcher>,
    /// fast-glob 3.2 matches nothing for a trailing `/`.
    matches_nothing: bool,
    /// A static pattern with a trailing `/`, whose entry path keeps it.
    slash_entry: bool,
    /// Whether picomatch also matches the pattern's text followed by `/`.
    optional_slash: bool,
    /// Matches a path with a trailing `/`, such an entry's path.
    entry_slash: Matcher,
}

/// Evaluates a declaration with one release's semantics.
pub(crate) fn evaluate(
    semantics: WorkspaceSemantics,
    tree: &Tree,
    files: &mut dyn Files,
    root: &Facts,
    declaration: &Declaration,
    bounds: Bounds,
    fold_case: bool,
) -> Result<Outcome, LoadError> {
    let mut outcome = Outcome::default();
    let mut work = Work::new(bounds.max_match_work);
    let mut evaluator = Evaluator {
        semantics,
        tree,
        rules: semantics.rules(fold_case),
        bounds,
    };
    if semantics == WorkspaceSemantics::YarnClassic
        && !root.private
        && !declaration.patterns.is_empty()
    {
        outcome.failures.insert(Failure {
            kind: FailureKind::RootNotPrivate,
            directory: None,
        });
    }
    let result = evaluator.workspace(&[], declaration, files, &mut outcome, &mut work);
    match result {
        Ok(()) => {}
        Err(Stop::Exhausted) => outcome.exhausted = true,
        Err(Stop::Load(error)) => return Err(error),
    }
    if !outcome.exhausted {
        evaluator.check_names(files, &mut outcome)?;
    }
    Ok(outcome)
}

enum Stop {
    Exhausted,
    Load(LoadError),
}

impl From<Exhausted> for Stop {
    fn from(_: Exhausted) -> Self {
        Self::Exhausted
    }
}

impl From<LoadError> for Stop {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

struct Evaluator<'a> {
    semantics: WorkspaceSemantics,
    tree: &'a Tree,
    rules: Rules,
    bounds: Bounds,
}

/// The directory names that fast-glob receives as `ignore` from Yarn 2 and
/// later: `**/node_modules`, `**/.git`, and `**/.yarn`. Their basenames are
/// static, so they also stop fast-glob from reading such directories.
const BERRY_IGNORED: [&str; 3] = ["node_modules", ".git", ".yarn"];

impl Evaluator<'_> {
    const fn berry(&self) -> bool {
        matches!(
            self.semantics,
            WorkspaceSemantics::YarnBerryV2 | WorkspaceSemantics::YarnBerryV4
        )
    }

    /// Rejects forms that this release matches in a way that is not
    /// modeled, as observed against the reference releases.
    fn quirk(&self, pattern: &Pattern) -> bool {
        // glob 13 selects nothing for `**/.name`, and picomatch nothing for
        // `a/**/.name`, while both walk other dotted names after `**`.
        let dot_after_globstar = pattern
            .segments
            .iter()
            .position(|segment| *segment == Segment::Globstar)
            .is_some_and(|first| {
                pattern.texts[first + 1..]
                    .iter()
                    .any(|text| text.starts_with('.'))
            });
        // pnpm 11 matches negated classes in negations inconsistently.
        let negated_class = pattern.negated && pattern.texts.iter().any(|text| text.contains("[!"));
        match self.semantics {
            WorkspaceSemantics::PnpmV11 => dot_after_globstar || negated_class,
            WorkspaceSemantics::Npm
            | WorkspaceSemantics::YarnBerryV4
            | WorkspaceSemantics::PnpmV10 => dot_after_globstar,
            WorkspaceSemantics::YarnBerryV2 => {
                // Yarn 2.4.3 and 3.8.7 match classes differently, and
                // picomatch's `?` before a trailing `**` is not observed.
                let class = pattern.texts.iter().any(|text| text.contains('['));
                let question = pattern.segments.last() == Some(&Segment::Globstar)
                    && pattern.texts.len() >= 2
                    && pattern.texts[pattern.texts.len() - 2].ends_with('?');
                // They also differ for `./` and for a static pattern with a
                // trailing `/`.
                let static_slash = pattern.trailing_slash && pattern.is_literal();
                // Yarn 2.4.3 lets a negation's trailing `**` after a `*`
                // match no segment, Yarn 3.8.7 does not.
                let star_globstar = pattern.negated
                    && pattern.segments.last() == Some(&Segment::Globstar)
                    && pattern.texts.len() >= 2
                    && pattern.texts[pattern.texts.len() - 2].ends_with('*');
                dot_after_globstar
                    || class
                    || question
                    || pattern.dot_slash
                    || static_slash
                    || star_globstar
            }
            WorkspaceSemantics::YarnClassic
            | WorkspaceSemantics::PnpmV12
            | WorkspaceSemantics::Bun => false,
        }
    }

    /// The fast-glob forms of a pattern, or `None` beyond the alternative
    /// limit.
    fn forms(&self, pattern: &Pattern) -> Option<Vec<Form>> {
        let v4 = self.semantics == WorkspaceSemantics::YarnBerryV4;
        // picomatch lets a trailing `**` match no segment unless a `*`
        // precedes it; fast-glob 3.3 also retries a directory with a `/`.
        let nonempty = |texts: &[String]| {
            texts.len() >= 2
                && texts[texts.len() - 1] == "**"
                && texts[texts.len() - 2].ends_with('*')
                && texts[texts.len() - 2] != "**"
        };
        let mut forms = Vec::new();
        let expansions = if v4 {
            pattern.expansions(self.bounds.parse.max_alternatives)?
        } else {
            vec![super::pattern::Expansion {
                segments: pattern.segments.clone(),
                texts: pattern.texts.iter().map(|text| text.to_string()).collect(),
            }]
        };
        for expansion in expansions {
            let literal = expansion
                .segments
                .iter()
                .map(Segment::literal)
                .collect::<Option<Vec<String>>>();
            let trailing_nonempty = nonempty(&expansion.texts);
            let entry_rules = Rules {
                dot: v4 && pattern.negated,
                trailing_globstar_nonempty: !v4 && trailing_nonempty,
                ..self.rules
            };
            let depth_rules = Rules {
                trailing_globstar_nonempty: trailing_nonempty,
                ..self.rules
            };
            let last = expansion.segments.last();
            let depth = pattern.negated
                && (matches!(last, Some(Segment::Globstar)) && expansion.segments.len() >= 2
                    || last.and_then(Segment::literal).is_some());
            let depth = (depth && !pattern.trailing_slash)
                .then(|| Matcher::from_segments(&expansion.segments, depth_rules));
            forms.push(Form {
                literal,
                base: expansion.texts
                    [..super::pattern::glob_parent(&expansion.texts, pattern.trailing_slash)]
                    .to_vec(),
                entry: Matcher::from_segments(&expansion.segments, entry_rules),
                depth,
                matches_nothing: !v4
                    && pattern.trailing_slash
                    && (pattern.negated || !pattern.is_literal()),
                slash_entry: pattern.trailing_slash && !pattern.negated && pattern.is_literal(),
                // picomatch accepts a trailing `/` after a pattern whose
                // last segment is `*`; other endings in `*` or a class are
                // rejected next to a static `x/`.
                optional_slash: expansion.texts.last().is_some_and(|text| text == "*"),
                entry_slash: {
                    let mut segments = expansion.segments.clone();
                    if pattern.trailing_slash {
                        segments.push(Segment::Alternatives(vec![Vec::new()]));
                    }
                    Matcher::from_segments(&segments, entry_rules)
                },
            });
        }
        Some(forms)
    }

    /// Evaluates the declaration of the workspace at `base`, and for Yarn 2
    /// and later the declarations of its members.
    fn workspace(
        &mut self,
        base: &[Box<str>],
        declaration: &Declaration,
        files: &mut dyn Files,
        outcome: &mut Outcome,
        work: &mut Work,
    ) -> Result<(), Stop> {
        if !declaration.complete {
            outcome.incomplete_declaration = true;
        }
        let mut prepared = Vec::new();
        let mut rejected = false;
        let mut seen = BTreeSet::new();
        for input in &declaration.patterns {
            // globby removes repeated patterns before pairing each with the
            // negations after it.
            if self.semantics == WorkspaceSemantics::YarnBerryV2 && !seen.insert(&*input.text) {
                continue;
            }
            let parsed = parse(&input.text, self.semantics.negation(), self.bounds.parse).and_then(
                |parsed| match parsed {
                    Parsed::Pattern(pattern) if self.quirk(&pattern) => {
                        Err(PatternRejection::Quirk)
                    }
                    Parsed::Pattern(_) | Parsed::Root => Ok(parsed),
                },
            );
            match parsed {
                Ok(Parsed::Root) => {}
                Ok(Parsed::Pattern(pattern)) => {
                    let forms = if self.berry() {
                        match self.forms(&pattern) {
                            Some(forms) => forms,
                            None => {
                                outcome.rejected.push((
                                    input.location.clone(),
                                    PatternRejection::TooManyAlternatives,
                                ));
                                rejected = true;
                                continue;
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    // Bun walks `!a/b` as the names other than `a`, then `b`;
                    // `!./a` starts with the component `!.`, which every
                    // other name matches.
                    let negated_walk =
                        (self.semantics == WorkspaceSemantics::Bun && pattern.negated).then(|| {
                            if pattern.dot_slash {
                                let mut segments = vec![Segment::Alternatives(vec![vec![
                                    super::pattern::Token::Star,
                                ]])];
                                segments.extend(pattern.segments.iter().cloned());
                                Matcher::from_segments(&segments, self.rules)
                            } else {
                                Matcher::negated_first(&pattern, self.rules)
                            }
                        });
                    let matcher = Matcher::new(&pattern, self.rules);
                    prepared.push(Prepared {
                        pattern,
                        location: input.location.clone(),
                        matcher,
                        forms,
                        negated_walk,
                    });
                }
                Err(rejection) => {
                    outcome.rejected.push((input.location.clone(), rejection));
                    rejected = true;
                }
            }
        }
        if !rejected && self.semantics == WorkspaceSemantics::YarnBerryV4 {
            let uncertain = prepared.iter().any(|entry| {
                entry.pattern.negated
                    && entry.pattern.texts.last().is_some_and(|text| {
                        &**text != "*"
                            && &**text != "**"
                            && (text.ends_with('*') || text.ends_with(']'))
                    })
            });
            for entry in &prepared {
                let static_slash = !entry.pattern.negated
                    && entry.pattern.trailing_slash
                    && entry.pattern.is_literal();
                if uncertain && static_slash {
                    outcome
                        .rejected
                        .push((entry.location.clone(), PatternRejection::Quirk));
                    rejected = true;
                }
            }
        }
        if rejected {
            return Ok(());
        }
        let candidates = self.candidates(base, &prepared, outcome, work)?;
        self.reachable(base, &prepared, outcome, work)?;
        if self.semantics == WorkspaceSemantics::Bun {
            self.missing_listed(base, &prepared, &candidates, outcome);
        }
        let mut nested = Vec::new();
        for (index, direct) in candidates {
            let node = &self.tree.dirs[index];
            if let Some(member) = self.membership(node, direct, files, outcome)? {
                nested.push(member);
            }
        }
        if self.berry() {
            for index in nested {
                let node = &self.tree.dirs[index];
                let Read::Analyzed(facts) = files.manifest(&node.path)? else {
                    continue;
                };
                if let Some(declaration) = &facts.workspaces {
                    self.workspace(&node.segments, declaration, files, outcome, work)?;
                }
            }
        }
        Ok(())
    }

    /// The directories below `base` that the patterns select before
    /// manifests are checked, with whether a Bun listing named them
    /// directly.
    fn candidates(
        &self,
        base: &[Box<str>],
        prepared: &[Prepared],
        outcome: &mut Outcome,
        work: &mut Work,
    ) -> Result<Vec<(usize, bool)>, Stop> {
        let mut selected = Vec::new();
        let npm_negations = self.npm_negations(prepared, work)?;
        // fast-glob 3.3 reads every dynamic pattern from the root when one
        // of them has no base directory.
        let merged = self.semantics == WorkspaceSemantics::YarnBerryV4
            && prepared
                .iter()
                .filter(|entry| !entry.pattern.negated)
                .flat_map(|entry| &entry.forms)
                .any(|form| form.literal.is_none() && form.base.is_empty());
        for (index, node) in self.tree.dirs.iter().enumerate() {
            let Some(relative) = below(base, &node.segments) else {
                continue;
            };
            let units: Vec<Vec<u32>> = relative
                .iter()
                .map(|segment| units(segment, self.rules))
                .collect();
            let mut chosen = None;
            let (mut plain, mut dotted) = (false, false);
            for (position, entry) in prepared.iter().enumerate() {
                let selection = if entry.pattern.negated {
                    match &entry.negated_walk {
                        Some(walk) => {
                            self.bun_glob(prepared, position, walk, relative, &units, work)?
                        }
                        None => None,
                    }
                } else if self.berry() {
                    self.berry_selects(prepared, position, merged, relative, &units, work)?
                } else {
                    self.selects(prepared, position, relative, &units, &npm_negations, work)?
                };
                if let Some(direct) = selection {
                    chosen = Some(chosen.unwrap_or(false) || direct);
                    if entry.pattern.dot_slash {
                        dotted = true;
                    } else {
                        plain = true;
                    }
                }
            }
            if let Some(direct) = chosen {
                // Yarn 1 sees `./a/package.json` and `a/package.json` as two
                // files of one workspace name.
                if self.semantics == WorkspaceSemantics::YarnClassic && plain && dotted {
                    outcome.twice.insert(node.path.clone());
                }
                selected.push((index, direct));
            }
        }
        Ok(selected)
    }

    /// npm drops a negation that a later positive pattern matches as text
    /// (`appendNegatedPatterns`).
    fn npm_negations(&self, prepared: &[Prepared], work: &mut Work) -> Result<Vec<bool>, Stop> {
        let mut active = vec![false; prepared.len()];
        if self.semantics != WorkspaceSemantics::Npm {
            return Ok(active);
        }
        let text_rules = Rules {
            fold_case: false,
            ..self.rules
        };
        for (index, entry) in prepared.iter().enumerate() {
            if entry.pattern.negated {
                active[index] = true;
                continue;
            }
            let text = text_units(&entry.pattern, text_rules);
            for negation in 0..index {
                if active[negation] {
                    let matcher = text_matcher(&prepared[negation].pattern, text_rules);
                    if matcher.matches(&text, work)? {
                        active[negation] = false;
                    }
                }
            }
        }
        Ok(active)
    }

    /// Whether the positive pattern at `position` selects the directory
    /// for npm, Yarn 1, pnpm, or Bun, and whether it names it directly as a
    /// Bun listing.
    fn selects(
        &self,
        prepared: &[Prepared],
        position: usize,
        relative: &[Box<str>],
        units: &[Vec<u32>],
        npm_negations: &[bool],
        work: &mut Work,
    ) -> Result<Option<bool>, Stop> {
        let entry = &prepared[position];
        let pattern = &entry.pattern;
        let pruned = self.semantics.pruned_names();
        match self.semantics {
            WorkspaceSemantics::Npm => {
                if !entry.matcher.matches(units, work)? {
                    return Ok(None);
                }
                if relative.iter().any(|segment| pruned.contains(&&**segment)) {
                    return Ok(None);
                }
                let text_rules = Rules {
                    fold_case: false,
                    ..self.rules
                };
                let own = text_units(pattern, text_rules);
                let ignore_rules = Rules {
                    dot: true,
                    ..self.rules
                };
                for (index, negation) in prepared.iter().enumerate() {
                    if !npm_negations[index] {
                        continue;
                    }
                    // A remaining negation removes positive patterns that it
                    // matches as text.
                    let as_text = text_matcher(&negation.pattern, text_rules);
                    if as_text.matches(&own, work)? {
                        return Ok(None);
                    }
                    // glob's ignore: the path itself, or a parent when the
                    // negation ends with `/**`.
                    let ignore = Matcher::new(&negation.pattern, ignore_rules);
                    if ignore.matches(units, work)? {
                        return Ok(None);
                    }
                    if let Some(Segment::Globstar) = negation.pattern.segments.last() {
                        let parent = Matcher::from_segments(
                            &negation.pattern.segments[..negation.pattern.segments.len() - 1],
                            ignore_rules,
                        );
                        for end in 1..units.len() {
                            if parent.matches(&units[..end], work)? {
                                return Ok(None);
                            }
                        }
                    }
                }
                Ok(Some(false))
            }
            WorkspaceSemantics::YarnClassic => {
                if !entry.matcher.matches(units, work)? {
                    return Ok(None);
                }
                // `<pattern>/node_modules/**/+(package.json|yarn.json)`.
                for (index, segment) in relative.iter().enumerate() {
                    if &**segment == "node_modules"
                        && entry.matcher.matches(&units[..index], work)?
                    {
                        return Ok(None);
                    }
                }
                Ok(Some(false))
            }
            WorkspaceSemantics::PnpmV10
            | WorkspaceSemantics::PnpmV11
            | WorkspaceSemantics::PnpmV12 => {
                if !entry.matcher.matches(units, work)? {
                    return Ok(None);
                }
                if self.semantics.prunes(relative) {
                    return Ok(None);
                }
                if self.semantics == WorkspaceSemantics::PnpmV12
                    && self.wax_walk_prunes(prepared, &entry.pattern, units, work)?
                {
                    return Ok(None);
                }
                // Negations exclude the manifest only, not the subtree;
                // pnpm 11 and 12 also match dot names.
                let negation_rules = Rules {
                    dot: self.semantics != WorkspaceSemantics::PnpmV10,
                    ..self.rules
                };
                for negation in prepared.iter().filter(|entry| entry.pattern.negated) {
                    let matcher = Matcher::new(&negation.pattern, negation_rules);
                    if matcher.matches(units, work)? {
                        return Ok(None);
                    }
                }
                Ok(Some(false))
            }
            WorkspaceSemantics::Bun => {
                if pattern.is_literal() {
                    let literal: Vec<String> = pattern
                        .segments
                        .iter()
                        .filter_map(Segment::literal)
                        .collect();
                    let same = literal.len() == relative.len()
                        && literal
                            .iter()
                            .zip(relative)
                            .all(|(literal, segment)| same_name(literal, segment, self.rules));
                    return Ok(same.then_some(true));
                }
                self.bun_glob(prepared, position, &entry.matcher, relative, units, work)
            }
            WorkspaceSemantics::YarnBerryV2 | WorkspaceSemantics::YarnBerryV4 => {
                Err(Stop::Load(LoadError::Internal))
            }
        }
    }

    /// Bun's glob walk with `matcher`, which later negations can exclude.
    fn bun_glob(
        &self,
        prepared: &[Prepared],
        position: usize,
        matcher: &Matcher,
        relative: &[Box<str>],
        units: &[Vec<u32>],
        work: &mut Work,
    ) -> Result<Option<bool>, Stop> {
        let pruned = self.semantics.pruned_names();
        if !matcher.matches(units, work)? {
            return Ok(None);
        }
        if relative.iter().any(|segment| pruned.contains(&&**segment)) {
            return Ok(None);
        }
        // Only later negations exclude a match, also a dot name; a trailing
        // `**` must match at least one segment, and a negation with a
        // trailing `/` or a leading `./` matches no directory.
        let negation_rules = Rules {
            dot: true,
            trailing_globstar_nonempty: true,
            ..self.rules
        };
        for negation in prepared[position + 1..].iter().filter(|entry| {
            entry.pattern.negated && !entry.pattern.trailing_slash && !entry.pattern.dot_slash
        }) {
            let matcher = Matcher::new(&negation.pattern, negation_rules);
            if matcher.matches(units, work)? {
                return Ok(None);
            }
        }
        Ok(Some(false))
    }

    /// Whether fast-glob returns the directory for the positive pattern at
    /// `position`: a static pattern by its path, a dynamic one when it lies
    /// below the pattern's base directory and no negation applies.
    fn berry_selects(
        &self,
        prepared: &[Prepared],
        position: usize,
        merged: bool,
        relative: &[Box<str>],
        units: &[Vec<u32>],
        work: &mut Work,
    ) -> Result<Option<bool>, Stop> {
        let v4 = self.semantics == WorkspaceSemantics::YarnBerryV4;
        let entry = &prepared[position];
        // globby passes only the negations after a pattern; fast-glob 3.2
        // matches nothing for a dynamic pattern with a trailing `/`.
        if entry.forms.iter().all(|form| form.matches_nothing) {
            return Ok(None);
        }
        let negations: Vec<&Form> = prepared
            .iter()
            .enumerate()
            .filter(|(index, negation)| negation.pattern.negated && (v4 || *index > position))
            .flat_map(|(_, negation)| &negation.forms)
            .collect();
        for form in &entry.forms {
            let start = match &form.literal {
                Some(literal) => {
                    let same = literal.len() == relative.len()
                        && literal
                            .iter()
                            .zip(relative)
                            .all(|(literal, segment)| same_name(literal, segment, self.rules));
                    if !same {
                        continue;
                    }
                    // A static pattern is looked up, not walked.
                    relative.len()
                }
                None => {
                    let base = if merged { &[][..] } else { &form.base[..] };
                    if relative.len() <= base.len()
                        || !base
                            .iter()
                            .zip(relative)
                            .all(|(base, segment)| same_name(base, segment, self.rules))
                        || !form.entry.matches(units, work)?
                    {
                        continue;
                    }
                    base.len() + 1
                }
            };
            let mut skipped = false;
            for end in start..relative.len() {
                // `**/node_modules` stops the walk unless a dot directory,
                // which its `**` does not cross, comes before.
                if BERRY_IGNORED.contains(&&*relative[end - 1])
                    && relative[..end - 1]
                        .iter()
                        .all(|segment| !segment.starts_with('.'))
                {
                    skipped = true;
                    break;
                }
                for negation in &negations {
                    if let Some(depth) = &negation.depth
                        && depth.matches(&units[..end], work)?
                    {
                        skipped = true;
                        break;
                    }
                }
                if skipped {
                    break;
                }
            }
            // A static `a/` yields the entry `a/`, which only negations that
            // match a trailing `/` exclude.
            let slashed: Vec<Vec<u32>>;
            let (entry_units, slash) = if form.slash_entry {
                slashed = units.iter().cloned().chain([Vec::new()]).collect();
                (&slashed[..], true)
            } else {
                (units, false)
            };
            // fast-glob 3.3 matches ignores against entries with `dot`, 3.2
            // without.
            let last = relative.len() - 1;
            let ignored_entry = BERRY_IGNORED.contains(&&*relative[last])
                && (v4
                    || relative[..last]
                        .iter()
                        .all(|segment| !segment.starts_with('.')));
            if skipped || (!slash && ignored_entry) {
                continue;
            }
            let mut excluded = false;
            for negation in &negations {
                let hit = if slash {
                    negation.entry_slash.matches(entry_units, work)?
                        || (negation.optional_slash && negation.entry.matches(units, work)?)
                } else {
                    !negation.matches_nothing && negation.entry.matches(units, work)?
                };
                if hit {
                    excluded = true;
                    break;
                }
            }
            if !excluded {
                return Ok(Some(false));
            }
        }
        Ok(None)
    }

    /// Whether fast-glob cannot reach an untraversed entry for a pattern:
    /// a static pattern names another path, or a built-in ignore below the
    /// base directory stops the walk.
    fn berry_unreachable(
        &self,
        entry: &Prepared,
        relative: &[Box<str>],
        units: &[Vec<u32>],
        work: &mut Work,
    ) -> Result<bool, Stop> {
        for form in &entry.forms {
            match &form.literal {
                Some(literal) => {
                    let below = literal.len() >= relative.len()
                        && literal
                            .iter()
                            .zip(relative)
                            .all(|(literal, segment)| same_name(literal, segment, self.rules));
                    if below {
                        return Ok(false);
                    }
                }
                None => {
                    let start = form.base.len().min(relative.len());
                    let ignored =
                        relative
                            .iter()
                            .enumerate()
                            .skip(start)
                            .any(|(index, segment)| {
                                BERRY_IGNORED.contains(&&**segment)
                                    && relative[..index]
                                        .iter()
                                        .all(|segment| !segment.starts_with('.'))
                            });
                    if !ignored && form.entry.could_match_below(units, work)? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    /// pnpm 12 hands negations that end in `**` unchanged to the wax walk
    /// of each glob, which matches them below the glob's literal prefix
    /// rather than the root, against directories and the manifest file.
    fn wax_walk_prunes(
        &self,
        prepared: &[Prepared],
        pattern: &Pattern,
        units: &[Vec<u32>],
        work: &mut Work,
    ) -> Result<bool, Stop> {
        let segments = &pattern.segments;
        let children = segments.len() >= 2
            && segments[..segments.len() - 1]
                .iter()
                .all(|segment| segment.literal().is_some())
            && segments[segments.len() - 1]
                == Segment::Alternatives(vec![vec![super::pattern::Token::Star]]);
        if pattern.is_literal() || children {
            return Ok(false);
        }
        let prefix = segments
            .iter()
            .take_while(|segment| segment.literal().is_some())
            .count();
        let rules = Rules {
            dot: true,
            ..self.rules
        };
        let file = super::matcher::units("package.json", rules);
        for negation in prepared.iter().filter(|entry| {
            entry.pattern.negated && entry.pattern.segments.last() == Some(&Segment::Globstar)
        }) {
            let matcher = Matcher::new(&negation.pattern, rules);
            // Observed against the walk prefix and against the root.
            for start in [prefix, 0] {
                let mut manifest: Vec<Vec<u32>> = units[start.min(units.len())..].to_vec();
                manifest.push(file.clone());
                if matcher.matches(&manifest, work)? {
                    return Ok(true);
                }
                for end in start + 1..=units.len() {
                    if matcher.matches(&units[start..end], work)? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Bun fails on a directory that `workspaces` lists without wildcards
    /// when the directory does not exist.
    fn missing_listed(
        &self,
        base: &[Box<str>],
        prepared: &[Prepared],
        candidates: &[(usize, bool)],
        outcome: &mut Outcome,
    ) {
        for entry in prepared {
            if entry.pattern.negated || !entry.pattern.is_literal() {
                continue;
            }
            let mut segments: Vec<Box<str>> = base.to_vec();
            segments.extend(
                entry
                    .pattern
                    .segments
                    .iter()
                    .filter_map(Segment::literal)
                    .map(Box::from),
            );
            let exists = candidates
                .iter()
                .any(|(index, _)| self.tree.dirs[*index].segments == segments);
            let hidden = self.tree.hidden.iter().any(|node| {
                segments.len() >= node.segments.len()
                    && segments[..node.segments.len()] == node.segments[..]
            });
            if exists || hidden {
                continue;
            }
            let directory =
                SourcePath::new(&segments.join("/"), frameshift_core::PathLimits::default()).ok();
            outcome.failures.insert(Failure {
                kind: FailureKind::MissingListed,
                directory,
            });
        }
    }

    /// Records entries that discovery did not traverse and that a pattern
    /// could reach.
    fn reachable(
        &self,
        base: &[Box<str>],
        prepared: &[Prepared],
        outcome: &mut Outcome,
        work: &mut Work,
    ) -> Result<(), Stop> {
        for node in &self.tree.hidden {
            let Some(relative) = below(base, &node.segments) else {
                continue;
            };
            let units: Vec<Vec<u32>> = relative
                .iter()
                .map(|segment| units(segment, self.rules))
                .collect();
            for entry in prepared {
                let matcher = match (&entry.negated_walk, entry.pattern.negated) {
                    (Some(walk), true) => walk,
                    (None, true) => continue,
                    (_, false) => &entry.matcher,
                };
                let literal = self.semantics == WorkspaceSemantics::Bun
                    && entry.pattern.is_literal()
                    && !entry.pattern.negated;
                let prunes = if self.berry() {
                    self.berry_unreachable(entry, relative, &units, work)?
                } else {
                    !literal
                        && self.semantics != WorkspaceSemantics::YarnClassic
                        && self.semantics.prunes(relative)
                };
                if prunes {
                    continue;
                }
                if matcher.could_match_below(&units, work)? {
                    outcome.hidden.insert(node.path.clone());
                    break;
                }
            }
        }
        Ok(())
    }

    /// Decides one candidate's membership from its manifests, returning
    /// its index when it is a member whose own declaration matters.
    fn membership(
        &self,
        node: &Node,
        direct: bool,
        files: &mut dyn Files,
        outcome: &mut Outcome,
    ) -> Result<Option<usize>, Stop> {
        let index = self
            .tree
            .dirs
            .binary_search_by(|candidate| candidate.path.cmp(&node.path))
            .map_err(|_| LoadError::Internal)?;
        let path = node.path.clone();
        match files.presence(&node.path, "package.json") {
            Presence::Hidden => {
                outcome
                    .undetermined
                    .insert(path, Undetermined::HiddenManifest);
                return Ok(None);
            }
            Presence::Absent => {
                let alternatives: &[&str] = match self.semantics {
                    WorkspaceSemantics::YarnClassic => &["yarn.json"],
                    WorkspaceSemantics::PnpmV10
                    | WorkspaceSemantics::PnpmV11
                    | WorkspaceSemantics::PnpmV12 => &["package.yaml", "package.json5"],
                    WorkspaceSemantics::Npm
                    | WorkspaceSemantics::YarnBerryV2
                    | WorkspaceSemantics::YarnBerryV4
                    | WorkspaceSemantics::Bun => &[],
                };
                if alternatives
                    .iter()
                    .any(|name| files.presence(&node.path, name) != Presence::Absent)
                {
                    outcome
                        .undetermined
                        .insert(path, Undetermined::UnreadManifest);
                } else if direct {
                    outcome.failures.insert(Failure {
                        kind: FailureKind::MissingListed,
                        directory: Some(path),
                    });
                }
                return Ok(None);
            }
            Presence::Admitted => {}
        }
        let facts = match files.manifest(&node.path)? {
            Read::NotAnalyzed => {
                outcome.failures.insert(Failure {
                    kind: FailureKind::InvalidManifest,
                    directory: Some(path.clone()),
                });
                outcome
                    .undetermined
                    .insert(path, Undetermined::InvalidManifest);
                return Ok(None);
            }
            Read::Analyzed(facts) => facts,
        };
        match self.semantics {
            WorkspaceSemantics::YarnClassic => {
                let present =
                    |value: &Option<Box<str>>| value.as_deref().is_some_and(|v| !v.is_empty());
                if !present(&facts.name) || !present(&facts.version) {
                    return Ok(None);
                }
            }
            WorkspaceSemantics::Bun => match facts.name.as_deref() {
                None => {
                    outcome.failures.insert(Failure {
                        kind: FailureKind::MissingName,
                        directory: Some(path),
                    });
                    return Ok(None);
                }
                Some("") => return Ok(None),
                Some(_) => {}
            },
            WorkspaceSemantics::Npm
            | WorkspaceSemantics::YarnBerryV2
            | WorkspaceSemantics::YarnBerryV4
            | WorkspaceSemantics::PnpmV10
            | WorkspaceSemantics::PnpmV11
            | WorkspaceSemantics::PnpmV12 => {}
        }
        if let Some(declaration) = &facts.workspaces
            && matches!(
                self.semantics,
                WorkspaceSemantics::YarnBerryV2 | WorkspaceSemantics::YarnBerryV4
            )
            && !declaration.complete
        {
            outcome
                .undetermined
                .insert(path.clone(), Undetermined::NestedUnknown);
        }
        if outcome.twice.contains(&path) {
            outcome.failures.insert(Failure {
                kind: FailureKind::DuplicateName,
                directory: Some(path.clone()),
            });
        }
        outcome.members.insert(path);
        Ok(Some(index))
    }

    /// Reports members that share a name where the release rejects that.
    fn check_names(&self, files: &mut dyn Files, outcome: &mut Outcome) -> Result<(), LoadError> {
        if self.semantics.reads_pnpm_workspace() {
            return Ok(());
        }
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        let members: Vec<SourcePath> = outcome.members.iter().cloned().collect();
        for path in members {
            let name = match files.manifest(&path)? {
                Read::Analyzed(facts) => match facts.name {
                    Some(name) if !name.is_empty() => name.to_string(),
                    // npm names a member after its directory; Yarn 2 and
                    // later generate a name.
                    _ if self.semantics == WorkspaceSemantics::Npm => path
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    _ => continue,
                },
                Read::NotAnalyzed => continue,
            };
            if seen.insert(name, ()).is_some() {
                outcome.failures.insert(Failure {
                    kind: FailureKind::DuplicateName,
                    directory: Some(path),
                });
            }
        }
        Ok(())
    }
}

fn below<'a>(base: &[Box<str>], segments: &'a [Box<str>]) -> Option<&'a [Box<str>]> {
    if segments.len() > base.len() && segments[..base.len()] == *base {
        Some(&segments[base.len()..])
    } else {
        None
    }
}

/// A negation as minimatch matches a pattern's text against it, where a
/// trailing `/` requires the text to end with `/`.
fn text_matcher(pattern: &Pattern, rules: Rules) -> TextMatcher {
    let mut segments = pattern.segments.clone();
    if pattern.trailing_slash {
        segments.push(Segment::Alternatives(vec![Vec::new()]));
    }
    // minimatch's text match: a trailing `**` needs a segment.
    let rules = Rules {
        trailing_globstar_nonempty: true,
        ..rules
    };
    TextMatcher(Matcher::from_segments(&segments, rules))
}

/// A negation as minimatch matches text against it.
struct TextMatcher(Matcher);

impl TextMatcher {
    /// minimatch accepts a text with a trailing `/` like the text without it.
    fn matches(&self, text: &[Vec<u32>], work: &mut Work) -> Result<bool, Exhausted> {
        if self.0.matches(text, work)? {
            return Ok(true);
        }
        match text.split_last() {
            Some((last, rest)) if last.is_empty() && !rest.is_empty() => self.0.matches(rest, work),
            _ => Ok(false),
        }
    }
}

/// A pattern's text as a path of literal names, as npm matches patterns
/// against each other.
fn text_units(pattern: &Pattern, rules: Rules) -> Vec<Vec<u32>> {
    let mut text = pattern.body.to_string();
    if pattern.trailing_slash {
        text.push('/');
    }
    text.split('/')
        .map(|segment| units(segment, rules))
        .collect()
}

fn same_name(literal: &str, segment: &str, rules: Rules) -> bool {
    if rules.fold_case {
        literal.to_lowercase() == segment.to_lowercase()
    } else {
        literal == segment
    }
}
