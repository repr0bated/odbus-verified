//! RAG ingestion pipeline for source content (live workspace files).
//!
//! Pipeline: source text → enrich → chunk → Voyage embed → Cozo upsert
//!
//! Repomix zip ingestion is retired (2026-09-19): the knowledge loop indexes
//! live source through `code_index` mode=source only.
//!
//! Each Cozo point payload carries rich metadata for hover display:
//!   repo, file_path, language, symbols, doc_comments, imports, tags,
//!   is_test, line_start, line_end, chunk_index, total_chunks, content_hash

use crate::semantic_index::SemanticIndex;
use crate::voyage::VoyageClient;
use anyhow::{Context, Result};
use op_cozo_store::{SemanticHit, SemanticPoint};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, future::Future, sync::OnceLock, time::Duration};
use tracing::{info, warn};

// ─── constants ───────────────────────────────────────────────────────────────

/// The fused voyage-4 retrieval set — one shared embedding space, so `_large`
/// and `_lite` collections compare directly. Best available tier per language:
/// go/python/rust were re-vectorized to voyage-4-large (see ../logs
/// vectorize_lsp_4large_*), the rest remain voyage-4-lite. Rust replaced its old
/// voyage-code-3 solo collection here.
pub const DEFAULT_VOYAGE4_COLLECTIONS: &[&str] = &[
    "repos_lsp_c_cpp_voyage_4_lite",
    "repos_lsp_go_voyage_4_large",
    "repos_lsp_java_voyage_4_lite",
    "repos_lsp_python_voyage_4_large",
    "repos_lsp_rust_voyage_4_large",
    "repos_lsp_typescript_voyage_4_lite",
    "repos_specs_docs_voyage_4_lite",
];
const CHUNK_LINES: usize = 80; // ~2 kB of code per chunk
const OVERLAP_LINES: usize = 12;
const VOYAGE_BATCH: usize = 32; // points per upsert batch
const VOYAGE_RATE_DELAY_MS: u64 = 120; // ms between Voyage calls

// ─── extracted file metadata ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FileMeta {
    pub language: &'static str,
    pub file_type: FileType,
    pub symbols: Vec<String>, // top-level pub items
    pub doc_comments: Vec<String>,
    pub imports: Vec<String>, // use / import / require
    pub tags: Vec<String>,    // semantic auto-tags
    pub is_test: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum FileType {
    #[default]
    Source,
    Test,
    Config,
    Docs,
    Build,
    Other,
}

impl FileType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Test => "test",
            Self::Config => "config",
            Self::Docs => "docs",
            Self::Build => "build",
            Self::Other => "other",
        }
    }
}

// ─── chunk ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Chunk {
    pub repo: String,
    pub file_path: String,
    pub meta: FileMeta,
    pub content: String,    // raw chunk lines
    pub embed_text: String, // metadata header + content (what gets embedded)
    pub content_hash: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub line_start: usize,
    pub line_end: usize,
}

// ─── ingestion stats ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct IngestStats {
    pub files_parsed: usize,
    pub chunks_created: usize,
    pub chunks_upserted: usize,
    pub chunks_skipped: usize,
    pub errors: usize,
}

// ─── public API ───────────────────────────────────────────────────────────────

/// RAG query result returned by `RagPipeline::query`.
#[derive(Debug, Clone, Serialize)]
pub struct RagResult {
    pub retrieval_collection: String,
    pub score: f32,
    pub repo: String,
    pub file_path: String,
    pub language: String,
    pub file_type: String,
    pub symbols: Vec<String>,
    pub doc_comments: Vec<String>,
    pub imports: Vec<String>,
    pub tags: Vec<String>,
    pub is_test: bool,
    pub line_start: i64,
    pub line_end: i64,
    pub chunk_index: i64,
    pub total_chunks: i64,
    pub content: String,
}

/// Structured filters for code-aware semantic retrieval.
///
/// `repo`/`language`/`file_type` are pushed into Cozo as server-side `must`
/// conditions. `path_contains`/`symbol_contains`/`exclude_tests` are applied
/// client-side after scoring, because they are substring/boolean predicates
/// over payload fields rather than exact keyword matches.
#[derive(Debug, Clone, Default)]
pub struct CodeFilter {
    pub repo: Option<String>,
    pub language: Option<String>,
    pub file_type: Option<String>,
    pub path_contains: Option<String>,
    pub symbol_contains: Option<String>,
    pub exclude_tests: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalMode {
    Completion,
    Search,
    Deep,
}

impl RetrievalMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "completion" | "complete" | "autocomplete" => Self::Completion,
            "deep" | "chat" | "edit" => Self::Deep,
            _ => Self::Search,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completion => "completion",
            Self::Search => "search",
            Self::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalProfile {
    pub mode: RetrievalMode,
    pub collections: Vec<String>,
    pub limit: u64,
    pub fetch_limit: u64,
    pub rerank_enabled: bool,
    pub kiro_lsp_state_dir: String,
    pub deadline: Duration,
}

impl RetrievalProfile {
    pub fn from_env(mode: RetrievalMode) -> Self {
        let limit = match mode {
            RetrievalMode::Completion => env_u64("COGNITIVE_COMPLETION_TOP_K", 12),
            RetrievalMode::Search => env_u64("COGNITIVE_SEARCH_TOP_K", 12),
            RetrievalMode::Deep => env_u64("COGNITIVE_DEEP_TOP_K", 50),
        }
        .clamp(1, 100);

        let fetch_limit = match mode {
            RetrievalMode::Completion => limit,
            RetrievalMode::Search => limit.saturating_mul(3).clamp(limit, 64),
            RetrievalMode::Deep => env_u64("COGNITIVE_DEEP_FETCH_K", 50).clamp(limit, 100),
        };

        let rerank_mode = std::env::var("COGNITIVE_RERANK_MODE").unwrap_or_else(|_| "auto".into());
        let rerank_enabled = match mode {
            RetrievalMode::Completion => rerank_mode == "always",
            RetrievalMode::Search => rerank_mode == "always",
            RetrievalMode::Deep => rerank_mode == "always" || rerank_mode == "auto",
        };
        let deadline = retrieval_deadline(mode);

        Self {
            mode,
            collections: default_collections_for_mode(mode),
            limit,
            fetch_limit,
            rerank_enabled,
            kiro_lsp_state_dir: std::env::var("COGNITIVE_KIRO_LSP_STATE_DIR")
                .unwrap_or_else(|_| "/home/jeremy/git/logs/kiro-lsp-state".into()),
            deadline,
        }
    }
}

impl CodeFilter {
    fn semantic_filters(&self) -> Vec<(String, String)> {
        [
            ("repo", &self.repo),
            ("language", &self.language),
            ("file_type", &self.file_type),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.as_ref().map(|value| (key.to_owned(), value.clone())))
        .collect()
    }

    fn post_matches(&self, r: &RagResult) -> bool {
        if self.exclude_tests && r.is_test {
            return false;
        }
        if let Some(p) = &self.path_contains
            && !r.file_path.to_lowercase().contains(&p.to_lowercase())
        {
            return false;
        }
        if let Some(s) = &self.symbol_contains {
            let needle = s.to_lowercase();
            let hit = r
                .symbols
                .iter()
                .any(|sym| sym.to_lowercase().contains(&needle))
                || r.file_path.to_lowercase().contains(&needle);
            if !hit {
                return false;
            }
        }
        true
    }
}

pub struct RagPipeline {
    index: SemanticIndex,
    voyage: VoyageClient,
}

impl RagPipeline {
    #[cfg(test)]
    pub(crate) fn for_test(index: SemanticIndex, voyage: VoyageClient) -> Self {
        Self { index, voyage }
    }

    pub fn from_env() -> Result<Self> {
        // Config (endpoint/model/key) resolves through the embedding_model
        // plugin's projection, falling back to its own env-var reads — see
        // op_plugins::state_plugins::embedding_model::voyage_embed_params.
        let voyage = VoyageClient::new()?;
        let index = SemanticIndex::from_env();

        Ok(Self { index, voyage })
    }

    pub(crate) async fn indexed_count(&self, collection: &str) -> Result<u64> {
        self.index.count(collection).await
    }

    pub(crate) fn embedding_model(&self) -> &str {
        self.voyage.model()
    }

    /// Semantic search over a Cozo collection.
    #[tracing::instrument(skip(self), fields(collection, query_text, limit))]
    pub async fn query(
        &self,
        collection: &str,
        query_text: &str,
        limit: u64,
        repo_filter: Option<&str>,
    ) -> Result<Vec<RagResult>> {
        let vector = self.embed_query(query_text).await?;

        let filters = repo_filter
            .map(|repo| vec![("repo".to_owned(), repo.to_owned())])
            .unwrap_or_default();
        let hits = self
            .index
            .search(collection, vector, limit, filters)
            .await?;
        Ok(hits.into_iter().map(point_to_result).collect())
    }

    /// Code-aware semantic search with structured filters.
    ///
    /// Server-side filters (`repo`, `language`, `file_type`) are pushed into
    /// Cozo; path/symbol/test filters are applied client-side after scoring.
    /// subid: `obs.service.code-rag.search@v1`
    #[tracing::instrument(skip(self, filter), fields(collection, query_text, limit))]
    pub async fn query_filtered(
        &self,
        collection: &str,
        query_text: &str,
        limit: u64,
        filter: &CodeFilter,
    ) -> Result<Vec<RagResult>> {
        let vector = self.embed_query(query_text).await?;
        let fetch = limit.saturating_mul(3).max(limit);

        let hits = self
            .index
            .search(collection, vector, fetch, filter.semantic_filters())
            .await?;
        let mut results: Vec<RagResult> = hits
            .into_iter()
            .map(point_to_result)
            .filter(|r| filter.post_matches(r))
            .collect();
        results.truncate(limit as usize);
        Ok(results)
    }

    /// Fused code retrieval: semantic score + lexical/symbol boost, then
    /// deduplicated to the single best-scoring chunk per file.
    ///
    /// This is the primary retrieval surface for the `code_context` tool: it
    /// avoids returning many chunks of the same file and surfaces files whose
    /// symbols or paths lexically match the query terms.
    /// subid: `obs.service.code-rag.fused-search@v1`
    #[tracing::instrument(skip(self, filter), fields(collection, query_text, limit))]
    pub async fn query_fused(
        &self,
        collection: &str,
        query_text: &str,
        limit: u64,
        filter: &CodeFilter,
    ) -> Result<Vec<RagResult>> {
        let vector = self.embed_query(query_text).await?;
        self.query_fused_with_vector(
            collection,
            query_text,
            limit,
            limit.saturating_mul(4).clamp(limit, 64),
            filter,
            &vector,
        )
        .await
    }

    /// Fused retrieval across compatible collections. The query is embedded
    /// once, then reused for each collection to keep completion requests cheap.
    pub async fn query_fused_collections(
        &self,
        collections: &[String],
        query_text: &str,
        limit: u64,
        fetch_limit: u64,
        filter: &CodeFilter,
        deadline: Duration,
    ) -> Result<Vec<RagResult>> {
        with_retrieval_deadline(
            deadline,
            self.query_fused_collections_unbounded(
                collections,
                query_text,
                limit,
                fetch_limit,
                filter,
            ),
        )
        .await
    }

    pub(crate) async fn query_fused_collections_unbounded(
        &self,
        collections: &[String],
        query_text: &str,
        limit: u64,
        fetch_limit: u64,
        filter: &CodeFilter,
    ) -> Result<Vec<RagResult>> {
        let vector = self.embed_query(query_text).await?;
        let mut all = Vec::new();
        let mut completed = 0usize;

        for collection in collections {
            match self
                .query_fused_with_vector(
                    collection,
                    query_text,
                    limit,
                    fetch_limit,
                    filter,
                    &vector,
                )
                .await
            {
                Ok(mut results) => {
                    completed += 1;
                    all.append(&mut results);
                }
                Err(err) => warn!(
                    collection = %collection,
                    error = %err,
                    "Code-RAG collection search failed"
                ),
            }
        }

        ensure_collection_success(completed, collections.len())?;

        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        dedupe_results_by_file(&mut all);
        all.truncate(limit as usize);
        Ok(all)
    }

    async fn query_fused_with_vector(
        &self,
        collection: &str,
        query_text: &str,
        limit: u64,
        fetch_limit: u64,
        filter: &CodeFilter,
        vector: &[f32],
    ) -> Result<Vec<RagResult>> {
        let fetch = fetch_limit.clamp(limit, 100);

        let hits = self
            .index
            .search(
                collection,
                vector.to_vec(),
                fetch,
                filter.semantic_filters(),
            )
            .await?;

        let terms: Vec<String> = query_text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2)
            .map(String::from)
            .collect();

        let mut best: HashMap<String, RagResult> = HashMap::new();
        for pt in hits {
            let mut r = point_to_result(pt);
            r.retrieval_collection = collection.to_string();
            if !filter.post_matches(&r) {
                continue;
            }

            // Lexical/symbol boost on top of the cosine score.
            let mut boost = 0.0f32;
            let sym = r.symbols.join(" ").to_lowercase();
            let path = r.file_path.to_lowercase();
            for t in &terms {
                if sym.contains(t) {
                    boost += 0.05;
                }
                if path.contains(t) {
                    boost += 0.03;
                }
            }
            r.score += boost.min(0.25);

            best.entry(r.file_path.clone())
                .and_modify(|e| {
                    if r.score > e.score {
                        *e = r.clone();
                    }
                })
                .or_insert(r);
        }

        let mut results: Vec<RagResult> = best.into_values().collect();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    /// Index a single in-memory source file (live workspace indexing).
    ///
    /// Takes raw file content directly so a changed buffer can be re-indexed
    /// straight from the workspace; enrichment, chunking, embedding and upsert
    /// are the single ingestion path.
    /// subid: `src.software.workspace.index@v1`
    #[tracing::instrument(skip(self, content), fields(repo, file_path, collection))]
    pub async fn ingest_source_text(
        &self,
        repo: &str,
        file_path: &str,
        content: &str,
        collection: &str,
    ) -> Result<IngestStats> {
        let lines: Vec<String> = content.lines().map(str::to_string).collect();
        let meta = enrich(file_path, &lines);
        let chunks = build_chunks(repo, file_path, meta, lines);

        let mut stats = IngestStats {
            files_parsed: 1,
            ..Default::default()
        };
        let mut batch: Vec<SemanticPoint> = Vec::new();

        for chunk in chunks {
            stats.chunks_created += 1;

            let vector = match self.embed_document(&chunk.embed_text).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(file = %chunk.file_path, error = %e, "Embed failed, skipping chunk");
                    stats.errors += 1;
                    continue;
                }
            };

            let payload = serde_json::json!({
                "repo":          chunk.repo,
                "file_path":     chunk.file_path,
                "language":      chunk.meta.language,
                "file_type":     chunk.meta.file_type.as_str(),
                "symbols":       chunk.meta.symbols,
                "doc_comments":  chunk.meta.doc_comments,
                "imports":       chunk.meta.imports,
                "tags":          chunk.meta.tags,
                "is_test":       chunk.meta.is_test,
                "line_start":    chunk.line_start,
                "line_end":      chunk.line_end,
                "chunk_index":   chunk.chunk_index,
                "total_chunks":  chunk.total_chunks,
                "content":       chunk.content,
                "content_hash":  chunk.content_hash,
            });
            batch.push(SemanticPoint {
                id: stable_uuid(&chunk.content_hash),
                vector,
                payload,
            });

            if batch.len() >= VOYAGE_BATCH {
                self.flush_batch(collection, &mut batch, &mut stats).await;
                tokio::time::sleep(Duration::from_millis(VOYAGE_RATE_DELAY_MS)).await;
            }
        }

        if !batch.is_empty() {
            self.flush_batch(collection, &mut batch, &mut stats).await;
        }

        if stats.chunks_upserted > 0
            && let Err(error) = self.index.publish_readiness(collection).await
        {
            warn!(%error, collection, "could not publish code index readiness");
        }

        info!(
            repo = %repo,
            file = %file_path,
            chunks = stats.chunks_created,
            upserted = stats.chunks_upserted,
            "Live source ingest complete"
        );

        Ok(stats)
    }

    // ─── private ─────────────────────────────────────────────────────────────

    async fn flush_batch(
        &self,
        collection: &str,
        batch: &mut Vec<SemanticPoint>,
        stats: &mut IngestStats,
    ) {
        let count = batch.len();
        match self.index.put(collection, std::mem::take(batch)).await {
            Ok(()) => stats.chunks_upserted += count,
            Err(error) => {
                warn!(%error, count, "Cozo vector batch write failed");
                stats.errors += 1;
            }
        }
    }

    async fn embed_document(&self, text: &str) -> Result<Vec<f32>> {
        self.voyage.embed(text, Some("document")).await
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.voyage.embed(text, Some("query")).await
    }
}

pub fn default_collection_from_env() -> String {
    // `COGNITIVE_REPOMIX_COLLECTION` is honoured only so an existing
    // deployment keeps pointing at the collection it already filled.
    std::env::var("COGNITIVE_RAG_COLLECTION")
        .or_else(|_| std::env::var("COGNITIVE_CODE_COLLECTION"))
        .or_else(|_| std::env::var("COGNITIVE_REPOMIX_COLLECTION"))
        .unwrap_or_else(|_| DEFAULT_VOYAGE4_COLLECTIONS[0].to_string())
}

pub fn default_collections_for_mode(mode: RetrievalMode) -> Vec<String> {
    let env_key = match mode {
        RetrievalMode::Completion => "COGNITIVE_COMPLETION_COLLECTIONS",
        RetrievalMode::Search => "COGNITIVE_SEARCH_COLLECTIONS",
        RetrievalMode::Deep => "COGNITIVE_DEEP_COLLECTIONS",
    };

    if let Ok(value) = std::env::var(env_key) {
        let collections = split_collection_list(&value);
        if !collections.is_empty() {
            return collections;
        }
    }

    if let Ok(value) = std::env::var("COGNITIVE_RAG_COLLECTIONS") {
        let collections = split_collection_list(&value);
        if !collections.is_empty() {
            return collections;
        }
    }

    match mode {
        RetrievalMode::Completion | RetrievalMode::Search => DEFAULT_VOYAGE4_COLLECTIONS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        RetrievalMode::Deep => vec![default_collection_from_env()],
    }
}

fn split_collection_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const MIN_RETRIEVAL_DEADLINE_MS: u64 = 100;
const MAX_RETRIEVAL_DEADLINE_MS: u64 = 120_000;

fn retrieval_deadline(mode: RetrievalMode) -> Duration {
    let key = match mode {
        RetrievalMode::Completion => "COGNITIVE_COMPLETION_DEADLINE_MS",
        RetrievalMode::Search => "COGNITIVE_SEARCH_DEADLINE_MS",
        RetrievalMode::Deep => "COGNITIVE_DEEP_DEADLINE_MS",
    };
    retrieval_deadline_from_values(
        mode,
        std::env::var(key).ok().as_deref(),
        std::env::var("COGNITIVE_RETRIEVAL_DEADLINE_MS")
            .ok()
            .as_deref(),
    )
}

fn retrieval_deadline_from_values(
    mode: RetrievalMode,
    mode_value: Option<&str>,
    global_value: Option<&str>,
) -> Duration {
    let default = match mode {
        RetrievalMode::Completion => 5_000,
        RetrievalMode::Search => 10_000,
        RetrievalMode::Deep => 30_000,
    };
    let value = mode_value
        .or(global_value)
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(default)
        .clamp(MIN_RETRIEVAL_DEADLINE_MS, MAX_RETRIEVAL_DEADLINE_MS);
    Duration::from_millis(value)
}

fn ensure_collection_success(completed: usize, total: usize) -> Result<()> {
    if completed == 0 && total != 0 {
        anyhow::bail!("all code retrieval collections failed");
    }
    Ok(())
}

pub(crate) async fn with_retrieval_deadline<F, T>(deadline: Duration, future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::time::timeout(deadline, future)
        .await
        .context("code retrieval deadline exceeded")?
}

fn dedupe_results_by_file(results: &mut Vec<RagResult>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    results.retain(|result| {
        let key = format!(
            "{}:{}:{}",
            result.retrieval_collection, result.repo, result.file_path
        );
        seen.insert(key)
    });
}

// ─── source enrichment ─────────────────────────────────────────────────────────

fn enrich(file_path: &str, lines: &[String]) -> FileMeta {
    let language = detect_language(file_path);
    let file_type = classify_file(file_path, lines);
    let is_test = file_type == FileType::Test
        || lines
            .iter()
            .any(|l| l.contains("#[test]") || l.contains("#[cfg(test)]"));

    let (symbols, doc_comments, imports) = match language {
        "rust" => extract_rust(lines),
        "typescript" | "javascript" => extract_ts(lines),
        "python" => extract_python(lines),
        "go" => extract_go(lines),
        _ => (vec![], vec![], vec![]),
    };

    let tags = auto_tags(file_path, &symbols, &imports, language);

    FileMeta {
        language,
        file_type,
        symbols,
        doc_comments,
        imports,
        tags,
        is_test,
    }
}

fn detect_language(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "proto" => "protobuf",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" | "mdx" => "markdown",
        "sh" | "bash" => "shell",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "java" => "java",
        "kt" => "kotlin",
        "nix" => "nix",
        _ => "text",
    }
}

fn classify_file(path: &str, lines: &[String]) -> FileType {
    let lower = path.to_lowercase();
    if lower.contains("test") || lower.contains("spec") || lower.ends_with("_test.rs") {
        return FileType::Test;
    }
    if lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
        || lower.ends_with("pyproject.toml")
        || lower.ends_with("go.mod")
        || lower.ends_with("build.rs")
        || lower.ends_with("makefile")
    {
        return FileType::Build;
    }
    if lower.ends_with(".md") || lower.ends_with(".rst") || lower.ends_with(".txt") {
        return FileType::Docs;
    }
    if lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".json")
        || lower.ends_with(".env")
    {
        return FileType::Config;
    }
    if lines
        .iter()
        .any(|l| l.contains("#[cfg(test)]") || l.contains("#[test]"))
    {
        return FileType::Test;
    }
    FileType::Source
}

// Rust symbol extraction
fn extract_rust(lines: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
    static RE_ITEM: OnceLock<Regex> = OnceLock::new();
    static RE_USE: OnceLock<Regex> = OnceLock::new();

    let re_item = RE_ITEM.get_or_init(|| {
        Regex::new(
            r"^\s*pub(?:\(crate\))?\s+(fn|struct|enum|trait|type|mod|const|static|impl)\s+(\w+)",
        )
        .expect("static regex pattern is valid")
    });
    let re_use = RE_USE.get_or_init(|| {
        Regex::new(r"^\s*use\s+([\w::{}, ]+);").expect("static regex pattern is valid")
    });

    let mut symbols = Vec::new();
    let mut doc_comments = Vec::new();
    let mut imports = Vec::new();
    let mut pending_doc: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with("///") || trimmed.starts_with("//!") {
            let doc = trimmed.trim_start_matches('/').trim().to_string();
            if !doc.is_empty() {
                pending_doc.push(doc);
            }
            continue;
        }

        if let Some(caps) = re_item.captures(trimmed) {
            let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let name = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            symbols.push(format!("{kind} {name}"));
            if !pending_doc.is_empty() {
                doc_comments.push(pending_doc.join(" "));
                pending_doc.clear();
            }
            continue;
        }

        if let Some(caps) = re_use.captures(trimmed) {
            let import = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            // Only keep top-level crate (first segment)
            let top = import.split("::").next().unwrap_or(&import);
            let top = top.trim_matches('{').trim().to_string();
            if !top.is_empty() && !imports.contains(&top) {
                imports.push(top);
            }
            continue;
        }

        pending_doc.clear();
    }

    // Cap to avoid huge payloads
    symbols.truncate(40);
    doc_comments.truncate(10);
    imports.truncate(30);

    (symbols, doc_comments, imports)
}

fn extract_ts(lines: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
    static RE_EXPORT: OnceLock<Regex> = OnceLock::new();
    static RE_IMPORT: OnceLock<Regex> = OnceLock::new();

    let re_export = RE_EXPORT.get_or_init(|| {
        Regex::new(r"^export\s+(?:default\s+)?(?:async\s+)?(?:function|class|interface|type|const|enum)\s+(\w+)").expect("static regex pattern is valid")
    });
    let re_import = RE_IMPORT.get_or_init(|| {
        Regex::new(r#"^import\s+.+from\s+['"]([^'"]+)['"]"#).expect("static regex pattern is valid")
    });

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut doc_comments = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(caps) = re_export.captures(trimmed) {
            symbols.push(caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string());
        }
        if let Some(caps) = re_import.captures(trimmed) {
            let src = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            let pkg = src.split('/').next().unwrap_or(&src);
            let pkg = pkg.trim_start_matches('@').to_string();
            if !imports.contains(&pkg) {
                imports.push(pkg);
            }
        }
        if trimmed.starts_with("/**") || trimmed.starts_with("* ") {
            let doc = trimmed.trim_start_matches(['/', '*', ' ']).to_string();
            if !doc.is_empty() {
                doc_comments.push(doc);
            }
        }
    }

    symbols.truncate(40);
    doc_comments.truncate(10);
    imports.truncate(30);
    (symbols, doc_comments, imports)
}

fn extract_python(lines: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
    static RE_DEF: OnceLock<Regex> = OnceLock::new();
    static RE_IMP: OnceLock<Regex> = OnceLock::new();

    let re_def = RE_DEF.get_or_init(|| {
        Regex::new(r"^(?:class|def|async def)\s+(\w+)").expect("static regex pattern is valid")
    });
    let re_imp = RE_IMP.get_or_init(|| {
        Regex::new(r"^(?:import|from)\s+([\w.]+)").expect("static regex pattern is valid")
    });

    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(caps) = re_def.captures(trimmed) {
            symbols.push(caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string());
        }
        if let Some(caps) = re_imp.captures(trimmed) {
            let pkg = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            let top = pkg.split('.').next().unwrap_or(&pkg).to_string();
            if !imports.contains(&top) {
                imports.push(top);
            }
        }
    }

    symbols.truncate(40);
    imports.truncate(30);
    (symbols, vec![], imports)
}

fn extract_go(lines: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
    static RE_DECL: OnceLock<Regex> = OnceLock::new();
    static RE_IMP: OnceLock<Regex> = OnceLock::new();

    let re_decl = RE_DECL.get_or_init(|| {
        Regex::new(r"^func\s+(?:\(\w+\s+\*?\w+\)\s+)?(\w+)|^type\s+(\w+)\s+(?:struct|interface)")
            .expect("static regex pattern is valid")
    });
    let re_imp = RE_IMP
        .get_or_init(|| Regex::new(r#"^\s+"([^"]+)""#).expect("static regex pattern is valid"));

    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(caps) = re_decl.captures(trimmed) {
            let name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                symbols.push(name);
            }
        }
        if let Some(caps) = re_imp.captures(trimmed) {
            let pkg = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let top = pkg.split('/').next_back().unwrap_or(pkg).to_string();
            if !imports.contains(&top) {
                imports.push(top);
            }
        }
    }

    symbols.truncate(40);
    imports.truncate(30);
    (symbols, vec![], imports)
}

fn auto_tags(path: &str, symbols: &[String], imports: &[String], lang: &str) -> Vec<String> {
    let mut tags: Vec<&str> = Vec::new();

    // Language tag
    tags.push(lang);

    // Path-based tags
    let lower = path.to_lowercase();
    for keyword in [
        "server", "client", "handler", "router", "auth", "error", "config", "test", "bench",
        "proto", "grpc", "http", "async", "stream", "channel", "database", "cache",
    ] {
        if lower.contains(keyword) {
            tags.push(keyword);
        }
    }

    // Symbol-based tags
    let sym_text = symbols.join(" ").to_lowercase();
    for keyword in [
        "trait", "impl", "async", "handler", "service", "client", "server", "error", "config",
        "builder", "stream",
    ] {
        if sym_text.contains(keyword) {
            tags.push(keyword);
        }
    }

    // Import-based tags
    for imp in imports {
        match imp.as_str() {
            "tokio" | "async_std" => tags.push("async"),
            "tonic" | "prost" => tags.push("grpc"),
            "axum" | "hyper" | "actix" | "warp" => tags.push("http"),
            "serde" | "serde_json" => tags.push("serialization"),
            "sqlx" | "diesel" | "sea_orm" => tags.push("database"),
            "tracing" | "log" => tags.push("logging"),
            "anyhow" | "thiserror" => tags.push("error-handling"),
            _ => {}
        }
    }

    let mut deduped: Vec<String> = Vec::new();
    for t in tags {
        let s = t.to_string();
        if !deduped.contains(&s) {
            deduped.push(s);
        }
    }
    deduped.truncate(20);
    deduped
}

// ─── chunking ─────────────────────────────────────────────────────────────────

fn build_chunks(repo: &str, file_path: &str, meta: FileMeta, lines: Vec<String>) -> Vec<Chunk> {
    if lines.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let step = CHUNK_LINES.saturating_sub(OVERLAP_LINES).max(1);
    let total = ((lines.len() as f64) / step as f64).ceil() as usize;
    let total = total.max(1);

    let mut idx = 0usize;
    let mut chunk_index = 0usize;
    while idx < lines.len() {
        let end = (idx + CHUNK_LINES).min(lines.len());
        let chunk_lines = &lines[idx..end];
        let content = chunk_lines.join("\n");
        let embed_text = build_embed_text(repo, file_path, &meta, &content);
        let content_hash =
            hex_hash(format!("{repo}:{file_path}:{chunk_index}:{content}").as_bytes());

        chunks.push(Chunk {
            repo: repo.to_string(),
            file_path: file_path.to_string(),
            meta: meta.clone(),
            content,
            embed_text,
            content_hash,
            chunk_index,
            total_chunks: total,
            line_start: idx + 1,
            line_end: end,
        });

        idx += step;
        chunk_index += 1;

        if end >= lines.len() {
            break;
        }
    }

    chunks
}

/// Build the text that gets embedded — metadata header + content.
/// The header primes the embedding model with structural context.
fn build_embed_text(repo: &str, file_path: &str, meta: &FileMeta, content: &str) -> String {
    let mut header = format!("REPO: {repo}\nFILE: {file_path}\nLANG: {}\n", meta.language);

    if !meta.symbols.is_empty() {
        header.push_str(&format!("SYMBOLS: {}\n", meta.symbols.join(", ")));
    }
    if let Some(first_doc) = meta.doc_comments.first() {
        header.push_str(&format!("DOCS: {}\n", first_doc));
    }
    if !meta.imports.is_empty() {
        header.push_str(&format!("DEPS: {}\n", meta.imports.join(", ")));
    }
    if !meta.tags.is_empty() {
        header.push_str(&format!("TAGS: {}\n", meta.tags.join(", ")));
    }
    if meta.is_test {
        header.push_str("TYPE: test\n");
    }

    format!("{header}---\n{content}")
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn hex_hash(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn stable_uuid(hash: &str) -> String {
    // Use first 32 hex chars of hash to form a UUID-like stable ID
    if hash.len() >= 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &hash[0..8],
            &hash[8..12],
            &hash[12..16],
            &hash[16..20],
            &hash[20..32]
        )
    } else {
        uuid::Uuid::new_v4().to_string()
    }
}

/// Map a scored Cozo point and its metadata into a retrieval result.
fn point_to_result(pt: SemanticHit) -> RagResult {
    let p = pt.payload;
    RagResult {
        retrieval_collection: str_field(&p, "retrieval_collection"),
        score: pt.score,
        repo: str_field(&p, "repo"),
        file_path: str_field(&p, "file_path"),
        language: str_field(&p, "language"),
        file_type: str_field(&p, "file_type"),
        symbols: str_arr(&p, "symbols"),
        doc_comments: str_arr(&p, "doc_comments"),
        imports: str_arr(&p, "imports"),
        tags: str_arr(&p, "tags"),
        is_test: p["is_test"].as_bool().unwrap_or(false),
        line_start: p["line_start"].as_i64().unwrap_or(0),
        line_end: p["line_end"].as_i64().unwrap_or(0),
        chunk_index: p["chunk_index"].as_i64().unwrap_or(0),
        total_chunks: p["total_chunks"].as_i64().unwrap_or(1),
        content: str_field(&p, "content"),
    }
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn str_arr(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieval_deadline_has_mode_default_override_and_bounds() {
        assert_eq!(
            retrieval_deadline_from_values(RetrievalMode::Completion, None, None),
            Duration::from_secs(5)
        );
        assert_eq!(
            retrieval_deadline_from_values(RetrievalMode::Completion, None, Some("1")),
            Duration::from_millis(MIN_RETRIEVAL_DEADLINE_MS)
        );
        assert_eq!(
            retrieval_deadline_from_values(RetrievalMode::Completion, Some("999999"), None),
            Duration::from_millis(MAX_RETRIEVAL_DEADLINE_MS)
        );
    }

    #[tokio::test]
    async fn pending_retrieval_is_bounded_by_total_deadline() {
        let started = tokio::time::Instant::now();
        let error = with_retrieval_deadline(Duration::from_millis(10), async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("deadline exceeded"));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn empty_successful_collection_is_distinct_from_all_failures() {
        assert!(ensure_collection_success(1, 1).is_ok());
        assert!(ensure_collection_success(0, 1).is_err());
    }
}
