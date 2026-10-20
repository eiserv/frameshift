//! Workspace patterns of every modeled package manager release over
//! directory listings (`D-073`).
//!
//! The input holds patterns, one per line, an empty line, and listed
//! directories, one per line: `+path` has a named and versioned
//! `package.json`, `-path` one without a name or version, `~path` is a
//! directory that discovery did not traverse, and a plain path has no
//! manifest. A `+path|a,b` member declares the nested patterns `a` and `b`.
//! Every release must select members deterministically and independently
//! of the listing order, only among listed directories with a manifest,
//! with the name and version that it requires, nothing when a pattern is
//! rejected, and the same members under a smaller work bound unless it
//! reports that the bound was reached.

use frameshift_engine::foundation::{PathLimits, SourcePath};
use frameshift_engine::package::{
    ListedDirectory, ListedManifest, ListingSelection, WorkspaceLimits, WorkspaceSemantics,
    select_members,
};

use crate::targets;

/// Evaluates the listing with every release.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let (head, tail) = text.split_once("\n\n").unwrap_or((text, ""));
    let patterns: Vec<&str> = head.lines().collect();
    let mut listing = Vec::new();
    for (index, line) in tail.lines().enumerate() {
        let (flag, rest) = match line.chars().next() {
            Some(flag @ ('+' | '-' | '~')) => (Some(flag), &line[1..]),
            _ => (None, line),
        };
        let (path, nested) = rest.split_once('|').unwrap_or((rest, ""));
        let Ok(path) = SourcePath::new(path, PathLimits::default()) else {
            continue;
        };
        let manifest = match flag {
            Some('+') => Some(ListedManifest {
                name: Some(format!("fictional-{index}").into()),
                version: Some("1.0.0".into()),
                workspaces: (!nested.is_empty())
                    .then(|| nested.split(',').map(Box::from).collect()),
            }),
            Some('-') => Some(ListedManifest::default()),
            _ => None,
        };
        listing.push(ListedDirectory {
            path,
            traversed: flag != Some('~'),
            manifest,
        });
    }
    // Listed paths must be unique.
    let mut seen = std::collections::BTreeSet::new();
    listing.retain(|directory| seen.insert(directory.path.clone()));
    let limits = WorkspaceLimits::default();
    for release in WorkspaceSemantics::ALL {
        let select = |listing: &[ListedDirectory], limits| {
            select_members(release, true, &patterns, listing, limits)
                .map_err(|error| format!("{release:?}: {error}"))
        };
        let selection = select(&listing, limits)?;
        if select(&listing, limits)? != selection {
            return Err(format!("{release:?}: selection is not repeatable"));
        }
        let mut reversed = listing.clone();
        reversed.reverse();
        if select(&reversed, limits)? != selection {
            return Err(format!(
                "{release:?}: selection depends on the listing order"
            ));
        }
        check(release, &listing, &selection)?;
        let small = WorkspaceLimits {
            max_match_work: 64,
            ..limits
        };
        let bounded = select(&listing, small)?;
        if bounded.exact && selection.exact && bounded.members != selection.members {
            return Err(format!(
                "{release:?}: a smaller work bound changed the members"
            ));
        }
    }
    Ok(())
}

fn check(
    release: WorkspaceSemantics,
    listing: &[ListedDirectory],
    selection: &ListingSelection,
) -> Result<(), String> {
    if !selection.members.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!("{release:?}: members are not sorted and unique"));
    }
    if !selection.rejected.is_empty() && (selection.exact || !selection.members.is_empty()) {
        return Err(format!("{release:?}: a rejected pattern selected members"));
    }
    for member in &selection.members {
        let Some(directory) = listing.iter().find(|directory| &directory.path == member) else {
            return Err(format!("{release:?}: {member:?} is not listed"));
        };
        let Some(manifest) = directory.manifest.as_ref().filter(|_| directory.traversed) else {
            return Err(format!("{release:?}: {member:?} has no manifest"));
        };
        let named = manifest
            .name
            .as_deref()
            .is_some_and(|name| !name.is_empty());
        let versioned = manifest
            .version
            .as_deref()
            .is_some_and(|version| !version.is_empty());
        let required = match release {
            WorkspaceSemantics::YarnClassic => named && versioned,
            WorkspaceSemantics::Bun => named,
            WorkspaceSemantics::Npm
            | WorkspaceSemantics::YarnBerryV2
            | WorkspaceSemantics::YarnBerryV4
            | WorkspaceSemantics::PnpmV10
            | WorkspaceSemantics::PnpmV11
            | WorkspaceSemantics::PnpmV12 => true,
        };
        if !required {
            return Err(format!("{release:?}: {member:?} lacks a required field"));
        }
    }
    Ok(())
}
