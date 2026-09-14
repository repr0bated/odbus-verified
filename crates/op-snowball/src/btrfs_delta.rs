//! Native btrfs delta primitives: generation numbers, `find-new` deltas, and
//! received-subvolume detection.
//!
//! These are the reactive trigger surface for vector replication. An
//! incremental `btrfs send -p <parent> <new> | btrfs receive` lands a
//! subvolume whose *arrival* is the event; `find_new_since` then answers
//! "which files are new" from btrfs's own transaction ids, so nothing has to
//! poll, watch, or diff directory listings.
//!
//! `received_uuid` distinguishes a replica (non-empty `Received UUID`) from a
//! locally produced subvolume, so the same binary can tell which side of the
//! send it is running on.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Result of a `btrfs subvolume find-new` sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindNewDelta {
    /// Subvolume-relative paths modified after the requested generation,
    /// deduplicated and sorted (one file can report many extents).
    pub files: Vec<PathBuf>,
    /// The `transid marker was N` trailer: the generation to pass as
    /// `lastgen` on the next sweep.
    pub transid: u64,
}

/// Files changed in `subvolume` *strictly after* generation `already_indexed`.
///
/// Pass `0` for a full sweep. The returned `transid` is the checkpoint for the
/// next call — persist it, not a wall-clock timestamp.
pub async fn find_new_since(subvolume: &Path, already_indexed: u64) -> Result<FindNewDelta> {
    let min_transid = min_transid_arg(already_indexed);
    let output = Command::new("btrfs")
        .args(["subvolume", "find-new"])
        .arg(subvolume)
        .arg(min_transid.to_string())
        .output()
        .await
        .context("failed to execute `btrfs subvolume find-new`")?;

    if !output.status.success() {
        anyhow::bail!(
            "`btrfs subvolume find-new {} {}` failed: {}",
            subvolume.display(),
            min_transid,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(parse_find_new(&String::from_utf8_lossy(&output.stdout)))
}

/// `find-new`'s generation argument is the *minimum* transid it reports, i.e.
/// inclusive: asking for the marker it just returned re-reports the same files.
/// Callers hold "already indexed through N", so the query starts at N+1.
fn min_transid_arg(already_indexed: u64) -> u64 {
    already_indexed.saturating_add(1)
}

/// Current generation of a subvolume, from `btrfs subvolume show`.
pub async fn generation(subvolume: &Path) -> Result<u64> {
    let show = subvolume_show(subvolume).await?;
    parse_generation(&show).with_context(|| {
        format!(
            "no Generation field in `btrfs subvolume show {}`",
            subvolume.display()
        )
    })
}

/// When a subvolume (or snapshot) was created, per btrfs's own metadata.
///
/// The filesystem inode's birth/mtime cannot answer this for a snapshot: those
/// are inherited from the source subvolume, so every snapshot of one subvolume
/// looks equally old. Only `btrfs subvolume show` knows the snapshot's own
/// creation time, which is what any retention policy has to bucket on.
pub async fn creation_time(subvolume: &Path) -> Result<Option<DateTime<Utc>>> {
    let show = subvolume_show(subvolume).await?;
    Ok(parse_creation_time(&show))
}

/// `Received UUID` of a subvolume, or `None` when it was produced locally.
///
/// A non-`None` value means this subvolume arrived through `btrfs receive`,
/// i.e. this host is the replication target rather than the origin.
pub async fn received_uuid(subvolume: &Path) -> Result<Option<String>> {
    let show = subvolume_show(subvolume).await?;
    Ok(parse_received_uuid(&show))
}

async fn subvolume_show(subvolume: &Path) -> Result<String> {
    let output = Command::new("btrfs")
        .args(["subvolume", "show"])
        .arg(subvolume)
        .output()
        .await
        .context("failed to execute `btrfs subvolume show`")?;

    if !output.status.success() {
        anyhow::bail!(
            "`btrfs subvolume show {}` failed: {}",
            subvolume.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse `find-new` output. Each extent line ends with the subvolume-relative
/// path after the `flags <FLAGS>` field; splitting there (rather than taking
/// the last whitespace token) keeps paths containing spaces intact.
fn parse_find_new(stdout: &str) -> FindNewDelta {
    let mut files = BTreeSet::new();
    let mut transid = 0u64;

    for line in stdout.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("transid marker was ") {
            transid = rest.trim().parse().unwrap_or(transid);
            continue;
        }
        if !line.starts_with("inode ") {
            continue;
        }
        // `... gen 110097 flags NONE some/path.bin`
        let Some((_, after_flags)) = line.split_once(" flags ") else {
            continue;
        };
        let Some((_flags, path)) = after_flags.split_once(' ') else {
            continue;
        };
        if !path.is_empty() {
            files.insert(PathBuf::from(path));
        }
    }

    FindNewDelta {
        files: files.into_iter().collect(),
        transid,
    }
}

fn parse_generation(show: &str) -> Option<u64> {
    show.lines()
        .filter_map(|line| line.trim().strip_prefix("Generation:"))
        .find_map(|value| value.trim().parse().ok())
}

/// Parse `Creation time: 2026-08-08 07:11:42 +0000`.
fn parse_creation_time(show: &str) -> Option<DateTime<Utc>> {
    show.lines()
        .filter_map(|line| line.trim().strip_prefix("Creation time:"))
        .find_map(|value| {
            DateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S %z")
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
}

fn parse_received_uuid(show: &str) -> Option<String> {
    show.lines()
        .filter_map(|line| line.trim().strip_prefix("Received UUID:"))
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "-")
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIND_NEW: &str = "\
inode 257 file offset 0 len 4096 disk start 13631488 offset 0 gen 110097 flags NONE vec-000000000001.bin
inode 257 file offset 4096 len 4096 disk start 13635584 offset 0 gen 110097 flags NONE vec-000000000001.bin
inode 258 file offset 0 len 12 disk start 0 offset 0 gen 110098 flags INLINE vec-000000000002.bin
transid marker was 110099
";

    #[test]
    fn parses_find_new_deduped_and_sorted() {
        let delta = parse_find_new(FIND_NEW);
        assert_eq!(
            delta.files,
            vec![
                PathBuf::from("vec-000000000001.bin"),
                PathBuf::from("vec-000000000002.bin"),
            ],
            "multiple extents of one file must collapse to a single entry"
        );
        assert_eq!(delta.transid, 110099);
    }

    #[test]
    fn parses_find_new_with_no_changes() {
        let delta = parse_find_new("transid marker was 110096\n");
        assert!(delta.files.is_empty());
        assert_eq!(delta.transid, 110096);
    }

    #[test]
    fn query_starts_one_past_the_indexed_generation() {
        // Verified against btrfs-progs on the live chain: `find-new <sv> 111728`
        // still lists the files whose gen is exactly 111728.
        assert_eq!(min_transid_arg(111728), 111729);
        assert_eq!(min_transid_arg(0), 1, "a full sweep still starts at gen 1");
        assert_eq!(min_transid_arg(u64::MAX), u64::MAX, "must not wrap");
    }

    #[test]
    fn keeps_paths_containing_spaces() {
        let delta = parse_find_new(
            "inode 257 file offset 0 len 4096 disk start 0 offset 0 gen 5 flags NONE dir with space/v.bin\ntransid marker was 6\n",
        );
        assert_eq!(delta.files, vec![PathBuf::from("dir with space/v.bin")]);
    }

    #[test]
    fn parses_generation_and_received_uuid() {
        let show = "\
@/var/lib/opdbus/snowball/vectors
\tName: \t\t\tvectors
\tUUID: \t\t\t28ae1fdf-1601-eb4b-9345-fe76ea28e348
\tParent UUID: \t\t-
\tReceived UUID: \t\t-
\tCreation time: \t\t2026-08-08 07:11:42 +0000
\tGeneration: \t\t110096
";
        assert_eq!(parse_generation(show), Some(110096));
        assert_eq!(
            parse_creation_time(show).map(|dt| dt.to_rfc3339()),
            Some("2026-08-08T07:11:42+00:00".to_string()),
            "retention must bucket on the snapshot's own creation time"
        );
        assert_eq!(
            parse_creation_time("\tCreation time: \t\t2026-08-08 02:11:42 -0500")
                .map(|dt| dt.to_rfc3339()),
            Some("2026-08-08T07:11:42+00:00".to_string()),
            "non-UTC offsets must normalize"
        );
        assert_eq!(parse_creation_time("no such field"), None);
        assert_eq!(
            parse_received_uuid(show),
            None,
            "a locally produced subvolume reports '-'"
        );

        let replica = show.replace(
            "Received UUID: \t\t-",
            "Received UUID: \t\t9c1b7e64-1111-2222-3333-444455556666",
        );
        assert_eq!(
            parse_received_uuid(&replica).as_deref(),
            Some("9c1b7e64-1111-2222-3333-444455556666")
        );
    }
}
