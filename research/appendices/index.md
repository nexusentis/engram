---
title: "Appendices"
sidebar_position: 7
---

# Appendices

Reference materials and supporting documentation. These appendices provide the raw data, execution logs, and academic context behind the findings presented in the main narrative.

## Contents

- **[Full Benchmark History](./benchmark-history)** --- The complete run-by-run record of all 50+ benchmark experiments: every score, every delta, every category breakdown. The definitive reference for any claim made elsewhere in this documentation.

- **[Academic References](./papers)** --- Summaries of the key papers and systems that informed our design decisions, from the LongMemEval benchmark paper itself to Zep/Graphiti's bi-temporal model and EMem's elementary discourse units.

- **[Execution Log](./execution-log)** --- A condensed timeline of the plan execution from the first ingestion experiment through the GPT-5.2 ensemble: ingestion runs, retrieval experiments, B2/B3 post-mortems, model upgrade (Phase 6), ensemble router (Phase 7), productionization (Phase 8), quick wins (Phase 9), and Phases 10-11 — covering the full progression from 85.8% to 95.8% (#1 globally).

- **[ONNX Reranker Investigation](./onnx-reranker)** --- An investigation into 700 lines of dead code: an ONNX cross-encoder reranker that was never executed because the runtime library was never installed.

- **[Independent Review Prompts](./codex-prompts/)** --- Prompts used for independent code review sessions at key decision points.
