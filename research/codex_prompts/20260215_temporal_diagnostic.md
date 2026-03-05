# Review Prompt: Temporal Question Failure Diagnostic

## Task
Diagnose why our agentic memory system fails 5 out of 13 temporal questions on the LongMemEval-S benchmark. Produce a root cause analysis and concrete fixes.

## Context
- We score 42/50 (84%) overall, but Temporal is only 8/13 (62%) — worst category
- Non-agentic scores 10/13 on temporal — so 3 of 5 failures are agentic-specific
- The agentic loop has 7 tools: search_facts, search_messages, grep_messages, get_session_context, get_by_date_range, search_entity, done
- Questions have a `question_date` field which represents "today" for the user
- Date-grouped format shows conversations under `=== YYYY/MM/DD ===` headers

## The 5 Failing Questions

### Q7 (370a8ff4) — FAILS in BOTH agentic and non-agentic
- Question: "How many weeks had passed since I recovered from the flu when I went on my 10th jog outdoors?"
- Expected: 15
- Agentic answer: 12 weeks (wrong computation)
- Strategy detected: Enumeration (WRONG — this is Temporal)
- Iterations: 1 (answered from prefetch, no tool calls)
- question_date: 2023/10/15

### Q26 (9a707b81) — FAILS in agentic only, PASSES in non-agentic
- Question: "How many days ago did I attend a baking class at a local culinary school when I made my friend's birthday cake?"
- Expected: 21 days (22 including last day)
- Agentic answer: 5 days ago (wildly wrong)
- Strategy detected: Enumeration (WRONG — this is Temporal)
- Iterations: 1 (answered from prefetch, no tool calls)
- question_date: 2022/04/15

### Q36 (gpt4_93159ced) — FAILS in agentic only, PASSES in non-agentic
- Question: "How long have I been working before I started my current job at NovaTech?"
- Expected: 4 years and 9 months
- Agentic answer: 5 years (9 years total - 4 years at NovaTech)
- Strategy detected: Update
- Iterations: 4 (search_facts, search_messages, grep_messages, get_session_context)
- question_date: 2023/05/25

### Q39 (gpt4_f420262d) — FAILS in agentic only, PASSES in non-agentic
- Question: "What was the airline that I flied with on Valentine's day?"
- Expected: American Airlines
- Agentic answer: JetBlue (wrong airline)
- Strategy detected: Default (WRONG — should be Temporal)
- Iterations: 4 (search_facts, search_messages, get_by_date_range)
- question_date: 2023/03/02

### Q46 (eac54adc) — FAILS in BOTH agentic and non-agentic
- Question: "How many days ago did I launch my website when I signed a contract with my first client?"
- Expected: 19 days (20 including last day)
- Agentic answer: 24 days ago (wrong computation)
- Strategy detected: Enumeration (WRONG — this is Temporal)
- Iterations: 1 (answered from prefetch, no tool calls)
- question_date: 2023/03/25

## Observed Patterns
1. **3 of 5 answered in 1 iteration** (Q7, Q26, Q46) — agent decided prefetch was enough and called `done` immediately without using any temporal tools
2. **3 of 5 have wrong strategy** — "Enumeration" detected when question is clearly temporal (days-ago/weeks-ago calculation)
3. **All 5 involve date computation** — counting days/weeks/months between events
4. **Non-agentic passes 3 that agentic fails** — non-agentic sees the full retrieved context in one shot

## Code to Review
- Strategy detection: `crates/engram/src/bench/longmemeval/answerer.rs` — function `detect_question_strategy`
- Agentic system prompt: `crates/engram/src/bench/longmemeval/answerer.rs` — function `build_agent_system_prompt`
- Prefetch: `crates/engram/src/bench/longmemeval/answerer.rs` — function `prefetch_context`
- Tool result formatting: `crates/engram/src/bench/longmemeval/tools.rs`
- Date range tool: `crates/engram/src/bench/longmemeval/tools.rs` — `exec_get_by_date_range`
- Non-agentic prompt: `crates/engram/src/bench/longmemeval/answerer.rs` — `build_answer_prompt`

## Deliverable
Write analysis to `research/temporal_diagnostic_20260215.md` with:
1. Root cause for each of the 5 failures
2. Why strategy detection misclassifies temporal questions as Enumeration
3. Why agent stops after 1 iteration for date-computation questions
4. Concrete code fixes with file paths and line numbers
5. Expected impact of each fix
