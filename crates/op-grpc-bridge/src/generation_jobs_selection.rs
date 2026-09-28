//! Explicit private-generation selection for the existing StateSync transport.
//! This is path syntax only. Verified identity, exact sealed read admission and
//! owner activation are separate mandatory gates before any private SHM read.
use anyhow::{Result, ensure};

const ROOT: &str = "/org/opdbus/v1/plugins/tched_router";
const MAX_PATHS: usize = 32;
const MAX_PATH_BYTES: usize = 2048;
const MAX_SEGMENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationSelection {
    paths: Vec<String>,
}

fn private_path(path: &str) -> bool {
    ["generation_jobs"].into_iter().any(|child| {
        let root = format!("{ROOT}/{child}");
        path == root || path.starts_with(&(root + "/"))
    })
}

impl GenerationSelection {
    /// Empty/bare-plugin/public selectors remain public. Naming any private
    /// generation subtree opts into strict private mode; mixed requests reject.
    pub(crate) fn from_subscription(
        plugin_ids: &[String],
        paths: &[String],
    ) -> Result<Option<Self>> {
        if !paths.iter().any(|path| private_path(path)) {
            return Ok(None);
        }
        ensure!(
            plugin_ids.is_empty() || plugin_ids == ["tched_router"],
            "private generation subscriptions require one canonical plugin"
        );
        ensure!(
            !paths.is_empty() && paths.len() <= MAX_PATHS,
            "private generation subscriptions require 1..=32 explicit paths"
        );
        let mut canonical = Vec::with_capacity(paths.len());
        for path in paths {
            ensure!(
                private_path(path),
                "private and public state selectors cannot be mixed"
            );
            ensure!(
                path.len() <= MAX_PATH_BYTES && path.split('/').count() <= MAX_SEGMENTS,
                "private generation path exceeds its bounds"
            );
            ensure!(
                !path.chars().any(
                    |character| character.is_control() || matches!(character, '*' | '?' | '\\')
                ) && path
                    .split('/')
                    .skip(1)
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
                "private generation paths must name exact canonical descendants"
            );
            if !canonical.contains(path) {
                canonical.push(path.clone());
            }
        }
        Ok(Some(Self { paths: canonical }))
    }

    pub(crate) fn from_get_state(plugin_id: &str, path: &str) -> Result<Option<Self>> {
        Self::from_subscription(&[plugin_id.to_owned()], &[path.to_owned()])
    }

    pub(crate) fn paths(&self) -> &[String] {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_mode_requires_explicit_generation_path_and_never_a_bare_root() {
        for paths in [
            vec![],
            vec![ROOT.to_owned()],
            vec!["/org/opdbus/v1/plugins/*".into()],
        ] {
            assert!(
                GenerationSelection::from_subscription(&[], &paths)
                    .unwrap()
                    .is_none()
            );
        }
        let paths = vec![
            format!("{ROOT}/generation_jobs"),
            format!("{ROOT}/generation_jobs/one/spec/elements/title/props"),
        ];
        let selected = GenerationSelection::from_subscription(&[], &paths)
            .unwrap()
            .unwrap();
        assert_eq!(selected.paths(), paths);
        assert!(GenerationSelection::from_get_state("other", &paths[0]).is_err());
    }
    #[test]
    fn malformed_mixed_or_unbounded_selectors_fail_before_owner_lookup() {
        let root = format!("{ROOT}/generation_jobs");
        for paths in [
            vec![root.clone(), ROOT.into()],
            vec![root.clone(), "/org/opdbus/v1/plugins/config".into()],
            vec![format!("{root}/*")],
            vec![format!("{root}/one?")],
            vec![format!("{root}/../generation_jobs")],
            vec![format!("{root}//one")],
            vec![format!("{root}/one\n")],
            vec![format!("{root}/{}", "x".repeat(MAX_PATH_BYTES))],
            vec![root.clone(); MAX_PATHS + 1],
        ] {
            assert!(GenerationSelection::from_subscription(&[], &paths).is_err());
        }
        assert!(
            GenerationSelection::from_subscription(
                &["tched_router".into(), "config".into()],
                &[root]
            )
            .is_err()
        );
    }
}
