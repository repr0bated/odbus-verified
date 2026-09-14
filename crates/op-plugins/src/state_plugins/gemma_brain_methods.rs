//! Model-agnostic intent taxonomy for the private Gemma workspace.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

const MAX_TAGS: usize = 256;
const MAX_KEYWORDS: usize = 64;
const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GemmaTaxonomy {
    pub revision: u64,
    pub tags: BTreeMap<String, GemmaTag>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GemmaTag {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub keywords: Vec<String>,
    #[serde(default)]
    pub perspectives: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyInput {
    pub text: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagMatch {
    pub tag_id: String,
    pub score: u32,
    pub matched_keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyOutput {
    pub taxonomy_revision: u64,
    pub classification: Option<String>,
    pub matches: Vec<TagMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTaxonomyInput {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PerspectiveSummary {
    pub id: String,
    pub tag_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPerspectivesOutput {
    pub taxonomy_revision: u64,
    pub perspectives: Vec<PerspectiveSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTagsOutput {
    pub taxonomy_revision: u64,
    pub tags: Vec<GemmaTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterTagInput {
    pub expected_taxonomy_revision: u64,
    pub tag: GemmaTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterTagOutput {
    pub taxonomy_revision: u64,
    pub created: bool,
    pub tag: GemmaTag,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeIntentInput {
    pub text: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeIntentOutput {
    pub taxonomy_revision: u64,
    pub intent: Option<String>,
    pub matches: Vec<TagMatch>,
    pub perspectives: Vec<String>,
}

fn default_limit() -> u32 {
    8
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

pub fn validate_taxonomy(taxonomy: &GemmaTaxonomy) -> Result<()> {
    ensure!(
        taxonomy.tags.len() <= MAX_TAGS,
        "Gemma taxonomy exceeds tag capacity"
    );
    for (id, tag) in &taxonomy.tags {
        validate_tag(tag)?;
        ensure!(id == &tag.id, "Gemma taxonomy key differs from tag id");
    }
    Ok(())
}

pub fn validate_tag(tag: &GemmaTag) -> Result<()> {
    ensure!(valid_id(&tag.id), "Gemma tag id is invalid");
    ensure!(
        !tag.label.trim().is_empty() && tag.label.len() <= 128,
        "Gemma tag label is invalid"
    );
    ensure!(
        tag.description.len() <= 2048,
        "Gemma tag description is too large"
    );
    ensure!(
        !tag.keywords.is_empty() && tag.keywords.len() <= MAX_KEYWORDS,
        "Gemma tag requires 1..=64 keywords"
    );
    let mut keywords = BTreeSet::new();
    for keyword in &tag.keywords {
        ensure!(
            !keyword.trim().is_empty()
                && keyword.trim() == keyword
                && keyword.len() <= 128
                && !keyword.chars().any(char::is_control),
            "Gemma tag keyword is invalid"
        );
        ensure!(
            keywords.insert(keyword.to_lowercase()),
            "Gemma tag repeats a keyword"
        );
    }
    ensure!(
        tag.perspectives.len() <= 16,
        "Gemma tag has too many perspectives"
    );
    let mut perspectives = BTreeSet::new();
    for perspective in &tag.perspectives {
        ensure!(valid_id(perspective), "Gemma perspective id is invalid");
        ensure!(
            perspectives.insert(perspective),
            "Gemma tag repeats a perspective"
        );
    }
    Ok(())
}

pub fn register_tag(
    taxonomy: &mut GemmaTaxonomy,
    input: RegisterTagInput,
) -> Result<RegisterTagOutput> {
    validate_taxonomy(taxonomy)?;
    validate_tag(&input.tag)?;
    ensure!(
        taxonomy.revision == input.expected_taxonomy_revision,
        "stale Gemma taxonomy revision"
    );
    let created = !taxonomy.tags.contains_key(&input.tag.id);
    ensure!(
        !created || taxonomy.tags.len() < MAX_TAGS,
        "Gemma taxonomy tag capacity reached"
    );
    taxonomy.revision = taxonomy
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Gemma taxonomy revision overflow"))?;
    taxonomy
        .tags
        .insert(input.tag.id.clone(), input.tag.clone());
    Ok(RegisterTagOutput {
        taxonomy_revision: taxonomy.revision,
        created,
        tag: input.tag,
    })
}

pub fn list_tags(taxonomy: &GemmaTaxonomy) -> Result<ListTagsOutput> {
    validate_taxonomy(taxonomy)?;
    Ok(ListTagsOutput {
        taxonomy_revision: taxonomy.revision,
        tags: taxonomy.tags.values().cloned().collect(),
    })
}

pub fn list_perspectives(taxonomy: &GemmaTaxonomy) -> Result<ListPerspectivesOutput> {
    validate_taxonomy(taxonomy)?;
    let mut counts = BTreeMap::<String, u32>::new();
    for tag in taxonomy.tags.values() {
        for perspective in &tag.perspectives {
            *counts.entry(perspective.clone()).or_default() += 1;
        }
    }
    Ok(ListPerspectivesOutput {
        taxonomy_revision: taxonomy.revision,
        perspectives: counts
            .into_iter()
            .map(|(id, tag_count)| PerspectiveSummary { id, tag_count })
            .collect(),
    })
}

fn matches(taxonomy: &GemmaTaxonomy, text: &str, limit: u32) -> Result<Vec<TagMatch>> {
    validate_taxonomy(taxonomy)?;
    ensure!(
        !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES,
        "Gemma input text must be 1..=65536 bytes"
    );
    ensure!(
        (1..=64).contains(&limit),
        "Gemma result limit must be 1..=64"
    );
    let normalized = text.to_lowercase();
    let mut matches = taxonomy
        .tags
        .values()
        .filter_map(|tag| {
            let matched_keywords: Vec<_> = tag
                .keywords
                .iter()
                .filter(|keyword| normalized.contains(&keyword.to_lowercase()))
                .cloned()
                .collect();
            (!matched_keywords.is_empty()).then(|| TagMatch {
                tag_id: tag.id.clone(),
                score: u32::try_from(matched_keywords.len()).unwrap_or(u32::MAX),
                matched_keywords,
            })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.tag_id.cmp(&right.tag_id))
    });
    matches.truncate(limit as usize);
    Ok(matches)
}

pub fn classify(taxonomy: &GemmaTaxonomy, input: ClassifyInput) -> Result<ClassifyOutput> {
    let matches = matches(taxonomy, &input.text, input.limit)?;
    Ok(ClassifyOutput {
        taxonomy_revision: taxonomy.revision,
        classification: matches.first().map(|entry| entry.tag_id.clone()),
        matches,
    })
}

pub fn analyze_intent(
    taxonomy: &GemmaTaxonomy,
    input: AnalyzeIntentInput,
) -> Result<AnalyzeIntentOutput> {
    let matches = matches(taxonomy, &input.text, input.limit)?;
    let perspectives = matches
        .iter()
        .filter_map(|entry| taxonomy.tags.get(&entry.tag_id))
        .flat_map(|tag| tag.perspectives.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(AnalyzeIntentOutput {
        taxonomy_revision: taxonomy.revision,
        intent: matches.first().map(|entry| entry.tag_id.clone()),
        matches,
        perspectives,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: &str, keywords: &[&str], perspectives: &[&str]) -> GemmaTag {
        GemmaTag {
            id: id.into(),
            label: id.into(),
            description: String::new(),
            keywords: keywords.iter().map(|value| (*value).into()).collect(),
            perspectives: perspectives.iter().map(|value| (*value).into()).collect(),
        }
    }

    #[test]
    fn registered_taxonomy_drives_deterministic_classification_and_intent() {
        let mut taxonomy = GemmaTaxonomy::default();
        register_tag(
            &mut taxonomy,
            RegisterTagInput {
                expected_taxonomy_revision: 0,
                tag: tag("deploy", &["release", "deploy"], &["operational"]),
            },
        )
        .unwrap();
        register_tag(
            &mut taxonomy,
            RegisterTagInput {
                expected_taxonomy_revision: 1,
                tag: tag("review", &["review"], &["analytical"]),
            },
        )
        .unwrap();
        let result = classify(
            &taxonomy,
            ClassifyInput {
                text: "Review the release before deploy".into(),
                limit: 8,
            },
        )
        .unwrap();
        assert_eq!(result.classification.as_deref(), Some("deploy"));
        let intent = analyze_intent(
            &taxonomy,
            AnalyzeIntentInput {
                text: "review release".into(),
                limit: 8,
            },
        )
        .unwrap();
        assert_eq!(intent.perspectives, vec!["analytical", "operational"]);
    }

    #[test]
    fn invalid_or_stale_registration_never_mutates_taxonomy() {
        let mut taxonomy = GemmaTaxonomy::default();
        let before = taxonomy.clone();
        assert!(register_tag(
            &mut taxonomy,
            RegisterTagInput {
                expected_taxonomy_revision: 1,
                tag: tag("deploy", &["deploy"], &[]),
            },
        )
        .is_err());
        assert_eq!(taxonomy, before);
    }
}
