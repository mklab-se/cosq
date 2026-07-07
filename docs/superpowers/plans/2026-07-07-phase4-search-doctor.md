# Cosq 1.0 Phase 4 — Search, Doctor, Skill, Release Plan

> Executed inline. Spec §7–8. Spike findings (cosmos.rs API_VERSION comment):
> vector/FTS execute per-range + pk-scoped; VectorDistance projects a score
> (exact cross-partition merge); FullTextScore does not (approximate merge).

## Tasks
1. **`cosq search`** (`commands/search.rs` + client search support):
   - Card-driven detection: vector policy → embed query text via ailloy
     (`Client::from_config().embed_one`? verify ailloy embed API), build
     `SELECT TOP N ..., VectorDistance(c.<path>, @qv) AS _score FROM c ORDER BY VectorDistance(...)`;
     merge per-range results by _score (order asc for cosine distance...
     verify direction from live behavior: our spike returned score 1 for the
     identical vector on ORDER BY ascending? — the spike showed id 1 (score 1)
     first, so Cosmos cosine "distance" here is similarity-like: descending.
     Trust Cosmos's per-range ORDER BY: merge with the same comparator as the
     observed per-range order, then take TOP N.
   - Embedding-model match: card.embed_node or dims-match against ailloy
     config embed-capable nodes; confirm once (interactive), persist to
     profile.embed_models + card.
   - FTS-only → `ORDER BY RANK FullTextScore(...)` per-range, interleaved
     merge (round-robin by rank position), note "approximate across partitions"
     unless pk-scoped. Both → RRF (single-partition/scoped only; cross-partition
     hybrid warned + FTS-style interleave). Neither → CONTAINS fallback note.
   - Flags: `--mode vector|text|hybrid`, `--top N` (default 10), `--show-sql`,
     `--pk`, `--db/--container`. Shell `:search`.
2. **`cosq explain`** (`commands/explain.rs` + client metrics):
   - client: `query_with_metrics(...)` sets `x-ms-documentdb-populatequerymetrics: true`
     and `populateIndexMetrics: true` (body-level? header `x-ms-cosmos-populateindexmetrics`),
     captures `x-ms-documentdb-query-metrics` + index metrics from response.
   - decode metrics text (k=v;k=v pairs) into a table; index metrics JSON
     (base64? verify live) → utilized/potential indexes.
   - AI on → diagnosis + indexing-policy JSON snippet + scoping advice
     (generate_text with metrics + card). AI off → decoded metrics only.
   - Shell `:explain` re-runs the last SQL.
3. **Agent skill + docs**: regenerate `cosq ai skill` emit content for the
   1.0 surface; rewrite doc/ai-reference.md; README/INSTALL/CLAUDE.md;
   CHANGELOG 1.0.0 entry.
4. **Release**: version 1.0.0, gates, live end-to-end (recreate spike-vec
   container for search test with real ailloy embeddings, then delete),
   merge to main, tag v1.0.0, verify CI.
