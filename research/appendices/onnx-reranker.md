---
title: "ONNX Reranker Investigation"
sidebar_position: 4
---

# ONNX Reranker Investigation

**Date:** Day 8

## Summary

The codebase contained approximately 700 lines of ONNX cross-encoder reranker code in `crates/engram/src/retrieval/reranker.rs` that had **never been executed** in production or testing. The ONNX Runtime dynamic library (`libonnxruntime.dylib`) was never installed on the development machine, making the entire code path dead from the start.

## What It Was

- **Model:** ms-marco-MiniLM-L-6-v2, a cross-encoder trained on the MS MARCO web search dataset
- **Mechanism:** Takes (query, document) pairs, runs them through a transformer that attends to both simultaneously, and produces a relevance score. More accurate than bi-encoder (embedding) similarity but O(N) slower.
- **Config flag:** `enable_reranking` in `AnswererConfig`, defaulting to `false`

## Why It Was Never Tested

1. The ONNX Runtime dynamic library was never installed on the development machine.
2. `CrossEncoderReranker::from_config()` calls ONNX initialization, which panics without the library.
3. All 9 tests that instantiated the reranker or `RetrievalEngine` (which loads the reranker) failed with: `Failed to load ONNX Runtime dylib: dlopen failed`.
4. These failures were silently ignored during development.

## The Misleading Comment

The configuration contained this comment:

```rust
enable_reranking: false, // ms-marco cross-encoder hurts: trained for web search, not memory retrieval
```

This statement was **speculative, not empirical.** The ONNX reranker was never benchmarked. The -14pp result documented as "LLM reranking with truncated snippets is harmful" came from a completely different code path --- the LLM-based reranker (`enable_llm_reranking`), which uses gpt-4o-mini API calls.

## Three Distinct Reranking Code Paths

| Config Flag | Implementation | Ever Tested? | Result |
|-------------|---------------|-------------|--------|
| `enable_reranking` | ONNX ms-marco cross-encoder (local inference) | Never (library missing) | Unknown |
| `enable_llm_reranking` | gpt-4o-mini scoring per passage | Yes | -14pp (harmful) |
| `enable_cross_encoder_rerank` | gpt-4o-mini scoring per passage | Yes | -14pp (harmful) |

The LLM-based reranker failed because it used 200-character truncated snippets for scoring, which destroyed the context needed for relevance judgment. A proper cross-encoder with full text might have performed differently, but we never got to test that hypothesis.

## Tests Removed

Nine tests that had been silently failing due to the missing ONNX Runtime were removed:

**From `retrieval/reranker.rs`:**
- `test_rerank_depth_limit`
- `test_rerank_min_score_filter`

**From `retrieval/engine.rs`:**
- `test_engine_creation`
- `test_engine_decomposition_disabled`
- `test_engine_decomposition_enabled`
- `test_analyze_query`
- `test_execute_pipeline`
- `test_execute_decomposed_pipeline`
- `test_empty_result`

## Assessment

The ms-marco model is trained for web search relevance --- a distribution fundamentally different from personal memory retrieval. Even if the ONNX Runtime were installed and the reranker were functional, it would likely perform poorly on memory-style queries where relevance depends on personal context, temporal relationships, and entity identity rather than topical similarity.

The LLM-based reranker (which was tested) caused a -14pp regression despite having the advantage of being instruction-tunable. A static web-search model would likely fare worse. The ~700 lines of dead ONNX code were flagged for removal.
