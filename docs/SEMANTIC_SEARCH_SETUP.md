# Semantic Search Setup

## Overview

Semantic search uses hardcoded **Voyage AI embeddings** + **Qdrant vector database** to enable code and compliance repo search independent of the control plane model selection.

## Architecture

```
Repository (code/compliance)
    ↓ repomix XML export
    ↓ chunk (4000 chars, 400 char overlap)
    ↓ embed (voyage-context-4)
    ↓ upsert to Qdrant
        ↓ hover metadata in payload
            (line_start, line_end, repo, file, symbols, imports)
```

## Collection

**Name:** `repos_repomix_voyage_context_4`  
**Endpoint:** `http://10.200.0.2:6334` (gRPC)  
**Embedding Model:** voyage-context-4 (1024 dims)  
**Distance:** COSINE  

## Vectorization Scripts

### Code Repos
```bash
/srv/git/repos-bulk/vectorize_fixed.py
```
- Input: 47 repos in `/srv/git/repos-bulk/` (non-done/)
- Chunks: 4000 chars with 400 char overlap
- Batch: 20 chunks at a time via Voyage API
- Payload: repo, file, chunk_idx, total_chunks

### Compliance Repos
```bash
/srv/git/repos-bulk/vectorize_compliance.py
```
- Input: 83 repos in `/srv/git/repos-bulk/done/`
- Same chunking and embedding as code repos
- Type: compliance (metadata marker)

Both upsert to **same collection** → unified search.

## Semantic Search Plugin

**File:** `/srv/git/odbus-verified/crates/op-cognitive/src/semantic_search.rs`

### Methods

#### `search_code(query, limit) → Results`
Search across all repos (code + compliance)
- Input: query string, result limit
- Output: top-k search results with hover metadata
- Uses: `repos_repomix_voyage_context_4` collection

#### `search_compliance(query, limit) → Results`
Alternative compliance-specific search
- Same collection (marked with type: "compliance")
- Currently aliases to search_code

#### `list_collections() → Metadata`
List available collections and stats
- Returns: primary collection, embedding dims, model info

#### `index_file(repo, file_path, content) → Status`
Live indexing (not yet implemented)
- Placeholder for dynamic content addition

## Configuration

**Environment Variables:**
```bash
MONGO_VOYAGER          # Voyage API key (required)
QDRANT_URL             # Qdrant gRPC endpoint (default: http://10.200.0.2:6334)
```

**Voyage Endpoint:**
```
https://ai.mongodb.com/v1/contextualizedembeddings
```

**Payload Format (MongoDB endpoint):**
```json
{
  "model": "voyage-context-4",
  "inputs": [[text1], [text2], ...],  // List of lists!
  "input_type": "document"
}
```

## Hover Metadata

Each chunk's payload includes:
```json
{
  "repo": "repo_name",
  "file": "path/to/file.md",
  "chunk_idx": 0,
  "total_chunks": 100,
  "symbols": [
    {
      "name": "function_name",
      "kind": "function",
      "line_start": 42,
      "line_end": 50,
      "doc": "doc comment",
      "visibility": "public"
    }
  ],
  "imports": ["import x", "from y"]
}
```

Used for IDE tooltips and context display in search results.

## Running Vectorization

### Start Both Processes
```bash
cd /srv/git/repos-bulk
nohup python3 vectorize_fixed.py > vectorize_fixed_$(date +%s).log 2>&1 &
nohup python3 vectorize_compliance.py > vectorize_compliance_$(date +%s).log 2>&1 &
```

### Monitor Progress
```bash
ps aux | grep vectorize | grep -v grep
# Check Qdrant collection size
python3 -c "from qdrant_client import QdrantClient; qdrant = QdrantClient(url='http://10.200.0.2:6334', prefer_grpc=True); info = qdrant.get_collection('repos_repomix_voyage_context_4'); print(f'{info.points_count} vectors')"
```

### View Logs
```bash
tail -f /srv/git/repos-bulk/vectorize_*.log
```

## Integration with Chat UI

The chat interface (`door-chat-adapter.ts`) now includes graceful fallback when LLM is unavailable:
```
⚠️ Chat unavailable: [error]. Try semantic search on vectorized repos instead.
```

Semantic search results can be shown as:
- Search interface in UI
- Context injection into prompts
- Reference cards in chat sidebar

## Repos Included

### Code Repos (47)
- Language subfolders: rust/, cpp/
- Top-level projects
- Unified repomix representation per repo

### Compliance Repos (83)
- OWASP frameworks (Top10, ASVS, API Security)
- Security tools (checkov, trivy, OPA, falco)
- Cloud compliance (GCP, AWS, CloudFoundry)
- Best practices (Docker, Kubernetes, Terraform)
- Governance & workflows (Istio, Consul, Airflow)
- Security standards (NIST, CIS, Mozilla SSL)
- Data protection & privacy
- Compliance frameworks and templates

## Common Issues

### Voyage API 400 Error
**Problem:** `"inputs requires enable_auto_chunking=True or input_type='query'"`  
**Solution:** Use `inputs` as list of lists `[[text1], [text2]]`

### Qdrant Connection Warning
```
UserWarning: Failed to obtain server version. Unable to check client-server compatibility.
```
**This is non-fatal** — prefer_grpc=True is set, connection works.

### Missing Repomix XML
Some repos don't have repomix generated yet. Run:
```bash
cd /path/to/repo && repomix --no-gitignore
```

## Future Work

- [ ] Live indexing via `index_file()` method
- [ ] Symbol-aware search (language-specific)
- [ ] Adherence checking (compliance validation)
- [ ] Integration with chat UI search interface
- [ ] Multi-language support (currently unified)
- [ ] Cache warmup on startup
