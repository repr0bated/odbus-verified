//! Public authoring documentation imported into the json_render sealed schema.
//! Runtime readers consume the pinned blob; they never read these source files.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const FORMAT: &str = "threetched-ui-authoring-v1";
const INSTRUCTIONS: &str =
    include_str!("../../../../deploy/config/threetched-skills/default/ui-authoring/SKILL.md");
const SOURCES: &str =
    include_str!("../../../../deploy/config/threetched-skills/default/ui-authoring/sources.json");

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthoringDocument {
    /// Stable relative reference identifier, never a runtime file path.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-path@v1"))]
    pub path: String,
    /// Public upstream documentation or source reference. Any runtime link
    /// adaptations are recorded alongside the original upstream digest.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-content@v1"))]
    pub content: String,
    /// Public upstream repository owning this reference.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-repository@v1"))]
    pub repository: String,
    /// Exact upstream commit at capture, not a deployment assertion.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-revision@v1"))]
    pub revision: String,
    /// Path inside the upstream repository at that revision.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-source-path@v1"))]
    pub source_path: String,
    /// Captured content digest checked when preparing the schema.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-document-sha256@v1"))]
    pub sha256: String,
    /// Original upstream bytes when the runtime copy has adapted links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-upstream-sha256@v1"))]
    pub upstream_sha256: Option<String>,
    /// Description of the packaging-only changes to this document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-adaptation@v1"))]
    pub adaptation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiAuthoring {
    /// Version of this public documentation envelope.
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.authoring-format@v1"))]
    pub format: String,
    /// The canonical UI-authoring skill. Reading it does not grant tools.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-instructions@v1"))]
    pub instructions: String,
    /// All retained skill entrypoints and the shared reference set for every
    /// mode; supporting documents are selected through an admitted reader.
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.authoring-prompt-paths@v1"))]
    pub prompt_document_paths: Vec<String>,
    /// Public references pinned to the captured application versions.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.authoring-documents@v1"))]
    pub documents: Vec<AuthoringDocument>,
}

#[derive(Deserialize)]
struct Sources {
    format: String,
    files: Vec<Source>,
}

#[derive(Deserialize)]
struct Source {
    path: String,
    repository: String,
    commit: String,
    source_path: String,
    sha256: String,
    #[serde(default)]
    upstream_sha256: Option<String>,
    #[serde(default)]
    adaptation: Option<String>,
}

macro_rules! reference {
    ($path:literal) => {
        (
            concat!("references/upstream/", $path),
            include_str!(concat!(
                "../../../../deploy/config/threetched-skills/default/ui-authoring/references/upstream/",
                $path
            )),
        )
    };
}

macro_rules! skill {
    ($name:literal) => {
        (
            concat!("skills/", $name, "/SKILL.md"),
            include_str!(concat!(
                "../../../../deploy/config/threetched-skills/default/",
                $name,
                "/SKILL.md"
            )),
        )
    };
}

const DOCUMENTS: &[(&str, &str)] = &[
    reference!("json-render/svelte-guide.md"),
    reference!("json-render/catalog-types.ts"),
    reference!("json-render/renderer.ts"),
    reference!("json-render/schema.ts"),
    reference!("json-render/LICENSE"),
    reference!("svelte/runes.md"),
    reference!("svelte/state.md"),
    reference!("svelte/derived.md"),
    reference!("svelte/props.md"),
    reference!("svelte/markup.md"),
    reference!("svelte/render.md"),
    reference!("svelte/context.md"),
    reference!("svelte/lifecycle.md"),
    reference!("svelte/LICENSE.md"),
    reference!("kit/routing.md"),
    reference!("kit/adapter-static.md"),
    reference!("kit/single-page-apps.md"),
    reference!("kit/shallow-routing.md"),
    reference!("kit/LICENSE"),
    reference!("vite/build.md"),
    reference!("vite/env-and-mode.md"),
    reference!("vite/assets.md"),
    reference!("vite/LICENSE"),
    reference!("svelte-ai-tools/LICENSE"),
    skill!("svelte-code-writer"),
    skill!("vite"),
    skill!("core"),
    skill!("remotion"),
    skill!("remotion-best-practices"),
    skill!("web-design-guidelines"),
    skill!("building-components"),
    skill!("ai-sdk"),
    skill!("ai-elements"),
    skill!("streamdown"),
];

impl Default for UiAuthoring {
    fn default() -> Self {
        let sources: Sources = serde_json::from_str(SOURCES)
            .expect("the captured UI documentation manifest must parse");
        assert_eq!(sources.format, "3tched-ui-authoring-sources/1");
        assert_eq!(sources.files.len(), DOCUMENTS.len());
        let mut documents = Vec::with_capacity(DOCUMENTS.len());
        for &(path, content) in DOCUMENTS {
            let matching: Vec<_> = sources
                .files
                .iter()
                .filter(|item| item.path == path)
                .collect();
            assert_eq!(matching.len(), 1, "one provenance record required for {path}");
            let source = matching[0];
            assert_eq!(source.upstream_sha256.is_some(), source.adaptation.is_some());
            if let Some(hash) = &source.upstream_sha256 {
                assert!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
            }
            if let Some(note) = &source.adaptation {
                assert!(!note.trim().is_empty() && note.len() <= 1024);
            }
            assert_eq!(
                format!("{:x}", Sha256::digest(content.as_bytes())),
                source.sha256,
                "captured UI reference changed without provenance update: {path}"
            );
            documents.push(AuthoringDocument {
                path: path.into(),
                content: content.into(),
                repository: source.repository.clone(),
                revision: source.commit.clone(),
                source_path: source.source_path.clone(),
                sha256: source.sha256.clone(),
                upstream_sha256: source.upstream_sha256.clone(),
                adaptation: source.adaptation.clone(),
            });
        }
        let guide = Self {
            format: FORMAT.into(),
            instructions: INSTRUCTIONS.into(),
            prompt_document_paths: [
                "references/upstream/json-render/svelte-guide.md",
                "references/upstream/svelte/runes.md",
                "references/upstream/svelte/render.md",
                "references/upstream/kit/adapter-static.md",
                "references/upstream/vite/assets.md",
                "skills/svelte-code-writer/SKILL.md",
                "skills/vite/SKILL.md",
                "skills/core/SKILL.md",
                "skills/remotion/SKILL.md",
                "skills/remotion-best-practices/SKILL.md",
                "skills/web-design-guidelines/SKILL.md",
                "skills/building-components/SKILL.md",
                "skills/ai-sdk/SKILL.md",
                "skills/ai-elements/SKILL.md",
                "skills/streamdown/SKILL.md",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            documents,
        };
        assert!(guide.instructions.len() <= 16 * 1024);
        assert!(guide.documents.len() <= 40);
        assert!(
            guide
                .documents
                .iter()
                .map(|doc| doc.content.len())
                .sum::<usize>()
                <= 256 * 1024
        );
        let prompt_bytes = guide
            .prompt_document_paths
            .iter()
            .map(|path| {
                guide
                    .documents
                    .iter()
                    .find(|doc| &doc.path == path)
                    .expect("prompt reference must exist in sealed documents")
                    .content
                    .len()
            })
            .sum::<usize>()
            + guide.instructions.len();
        assert!(prompt_bytes <= 96 * 1024);
        guide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_defaults_retain_document_provenance_and_all_requested_skills() {
        let schema = super::super::json_render_schema();
        let seed = crate::projection_seed_state_from_schema(&schema);
        let state: super::super::JsonRenderState = simd_json::serde::from_owned_value(seed).unwrap();
        assert_eq!(state.ui_authoring.format, FORMAT);
        assert_eq!(state.ui_authoring.instructions, INSTRUCTIONS);
        assert_eq!(state.ui_authoring.documents.len(), DOCUMENTS.len());
        for doc in &state.ui_authoring.documents {
            assert_eq!(doc.revision.len(), 40);
            assert_eq!(
                format!("{:x}", Sha256::digest(doc.content.as_bytes())),
                doc.sha256
            );
        }
        assert!(state.ui_authoring.prompt_document_paths.len() < state.ui_authoring.documents.len());
        for &(path, _) in DOCUMENTS.iter().filter(|(path, _)| path.starts_with("skills/")) {
            assert!(state.ui_authoring.prompt_document_paths.iter().any(|selected| selected == path));
        }
        assert!(schema.fields["ui_authoring"].read_only);
    }
}
