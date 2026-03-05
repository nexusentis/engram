---
title: "Deep Dive: Mastra OM (94.87%)"
sidebar_position: 3
---

# Deep Dive: Mastra Observational Memory (94.87%)

**Source**: [mastra.ai/blog/observational-memory](https://mastra.ai/blog/observational-memory)
**Code**: [github.com/mastra-ai/mastra](https://github.com/mastra-ai/mastra) -- `/packages/memory/src/processors/observational-memory/`
**Workshop**: [github.com/mastra-ai/workshop-longmemeval](https://github.com/mastra-ai/workshop-longmemeval)
**RAG Blog**: [mastra.ai/blog/use-rag-for-agent-memory](https://mastra.ai/blog/use-rag-for-agent-memory)
**Docs**: [mastra.ai/docs/memory/observational-memory](https://mastra.ai/docs/memory/observational-memory)

---

## Core Insight

No vector DB, no retrieval. ALL memories live in the LLM context window as compressed text. Two background agents (Observer + Reflector) manage the compression lifecycle. For LongMemEval-S (~115K tokens raw), observations compress to ~3-30K tokens, fitting easily in modern context windows.

---

## Architecture

### The Two-Block Context Window

```
[OBSERVATIONS BLOCK - compressed, prioritized memories]
Date: Dec 4, 2025 (3 days ago)
* (09:15) User stated they have 3 kids: Emma (12), Jake (9), Lily (5)
* (09:16) User's anniversary is March 15
* (10:30) User working on auth refactor
* (11:30) User will visit parents this weekend. (meaning Dec 7-8, 2025 - yesterday, likely already happened)

[1 week later]

Date: Dec 11, 2025 (today)
* (09:15) Continued work on feature X

[RAW MESSAGES BLOCK - recent uncompressed messages]
User: Hey, I wanted to ask about...
Assistant: Sure, I can help with that...
```

### Three Agents

1. **Actor Agent**: Sees observations + raw messages. Does the actual work. Gets injected context with personalization instructions.
2. **Observer Agent**: Watches conversations, compresses into observations when raw messages > ~30K tokens. Uses gemini-2.5-flash.
3. **Reflector Agent**: Consolidates/compresses observations when they > ~40K tokens. Uses same model.

### Token Budgets

| Setting | Default | Purpose |
|---------|---------|---------|
| `observation.messageTokens` | 30,000 | Triggers Observer |
| `reflection.observationTokens` | 40,000 | Triggers Reflector |
| `observation.bufferTokens` | 0.2 (20%) | Async pre-computation buffer frequency |
| `blockAfter` | 1.2x | Safety: forces sync observation at 36K tokens |
| `bufferActivation` | 0.5 (50%) | Reflection buffer trigger point |
| `shareTokenBudget` | false | Allow messages to borrow from observation budget |

---

## Observation Format (from source code)

Priority emoji system (used during observation, stripped during context injection):
- **Red**: High priority -- explicit user facts, preferences, goals, critical context
- **Yellow**: Medium -- project details, learned info, tool results
- **Green**: Low -- minor details, uncertain observations

During context injection, yellow and green emojis are stripped (only red kept). This is done by `optimizeObservationsForContext()`:
```typescript
// Remove yellow and green emojis (keep red for critical items)
optimized = optimized.replace(/yellow_emoji\s*/g, '');
optimized = optimized.replace(/green_emoji\s*/g, '');
// Remove semantic tags, arrow indicators, clean up whitespace
```

Date grouping with relative time annotations added at context injection:
```
Date: Dec 4, 2025 (3 days ago)      <-- relative time added dynamically
* (14:30) User prefers direct answers
* (14:31) Working on feature X

[1 week later]                       <-- gap markers between non-consecutive dates

Date: Dec 11, 2025 (today)
* (09:15) Continued work on feature X
```

### Inline Temporal Expansion

At context injection time, `addRelativeTimeToObservations()` expands inline estimated dates:
```
BEFORE: User will visit parents this weekend. (meaning Dec 7-8, 2025)
AFTER:  User will visit parents this weekend. (meaning Dec 7-8, 2025 - yesterday, likely already happened)
```

For future-intent observations where the referenced date is now in the past, it adds "likely already happened" -- helping the LLM infer that planned actions were completed.

---

## Three-Date Model (Critical for Temporal)

Each observation has TWO potential timestamps (the "three-date" model):

1. **Beginning (ALWAYS)**: Time the statement was made `(14:30)`
2. **End (CONDITIONAL)**: Date being referenced, ONLY when there is a relative time reference
3. **Relative annotation (at injection)**: Added dynamically by `formatRelativeTime()` / `addRelativeTimeToObservations()`

Rules from the Observer prompt:
- ALWAYS include message timestamp at the beginning
- ONLY add `(meaning DATE)` at the END when you can provide an ACTUAL DATE
- DO NOT add end dates for present-moment statements or vague references ("recently", "lately", "soon")
- If observation contains MULTIPLE events, SPLIT into separate lines, EACH with its own date

Examples:
```
GOOD: (09:15) User's friend had a birthday party in March. (meaning March 20XX)
GOOD: (09:15) User will visit their parents this weekend. (meaning June 17-18, 20XX)
GOOD: (09:15) User prefers hiking in the mountains.      <-- no time reference, no end date
BAD:  (09:15) User prefers hiking. (meaning June 15 - today)  <-- don't repeat timestamp

-- Multi-event splitting:
BAD:  User will visit parents this weekend and go to dentist tomorrow.
GOOD: User will visit their parents this weekend. (meaning June 17-18, 20XX)
      User will go to the dentist tomorrow. (meaning June 16, 20XX)
```

---

## Observer Prompt (Full Details from Source Code)

The observer prompt exists in three variants (A/B testable via env vars):
- **Current** (~300 lines): Full detailed rules with examples
- **Legacy** (~200 lines, Jan 7 2026): Smaller, `OM_USE_LEGACY_PROMPT=1`
- **Condensed V3** (~45 lines): Principle-based, `OM_USE_CONDENSED_PROMPT=1`

### Key Observer Rules (from current prompt):

**1. Assertions vs Questions**
- User TELLS something -> assertion: "User stated has two kids"
- User ASKS something -> question: "User asked help with X"
- Distinguish from STATEMENTS OF INTENT: "I'm looking forward to X" -> "User stated they will X"
- **User assertions are AUTHORITATIVE** -- source of truth about their own life

**2. State Changes and Updates**
- Frame explicitly: "User will use the new method (replacing the old approach)"
- "User is switching from A to B"
- "User moved their stuff (no longer at previous location)"
- Make superseded information explicit

**3. Precise Action Verbs**
- Replace vague verbs: "getting" -> "subscribed to" / "purchased"
- "got" -> "purchased" / "received as gift" / "was given"
- Use assistant's more precise terminology when available

**4. Preserving Details in Lists/Recommendations**
- NOT "Assistant recommended 5 hotels"
- BUT "Assistant recommended: Hotel A (near station), Hotel B (pet-friendly), Hotel C (has pool)"
- Always preserve DISTINGUISHING DETAILS that make each item queryable

**5. Quantities and Counts**
- Always preserve how many: "Item A (4 units, size large), Item B (2 units, size small)"
- Numbers, measurements, percentages: "43.7% faster, memory from 2.8GB to 940MB"

**6. Names/Handles/Identifiers**
- Always preserve: @photographer_one (portraits), @photographer_two (landscapes)
- Specific identifiers are critical for later retrieval

**7. Role/Participation**
- NOT "User attended the event"
- BUT "User was a presenter at the event (presented on microservices)"

**8. Unusual Phrasing**
- Quote exact user terminology: User did a "movement session" (their term for exercise)

**9. Tool Call Handling**
- Observe what was called, why, and what was learned
- Group related tool calls with indentation:
```
* (14:33) Agent debugging auth issue
  * ran git status, found 3 modified files
  * viewed auth.ts:45-60, found missing null check
  * applied fix, tests now pass
```

**10. WHO/WHAT/WHERE/WHEN**
- Capture all dimensions: not just "User went on a trip" but who with, where, when, what happened

### Observer Output Structure

```xml
<observations>
Date: Dec 4, 2025
* RED (14:30) [observation]
* YELLOW (14:31) [observation]
</observations>

<current-task>
Primary: [current work]
Secondary: [pending tasks or "waiting for user"]
</current-task>

<suggested-response>
[hint for agent's next message]
</suggested-response>
```

---

## Reflector Prompt (Full Details from Source Code)

The Reflector receives the FULL Observer extraction instructions so it understands how observations were created. Key additions:

**Purpose**: "reflect on all the observations, re-organize and streamline them, draw connections and conclusions"

**Critical constraint**: "your reflections are THE ENTIRETY of the assistant's memory. Any information you do not add will be immediately forgotten."

**Consolidation rules**:
- Preserve dates/times (temporal context is critical)
- Combine related items ("agent called view tool 5 times on file x")
- Condense OLDER observations more aggressively, retain more detail for RECENT
- User assertions take precedence over questions

**Three compression levels**:
- Level 0: No compression guidance (first attempt)
- Level 1: "Slightly more compression" -- target 8/10 detail level
- Level 2: "Aggressive compression" -- target 6/10 detail level, heavily condense early observations

**Validation**: After reflection, system checks if `reflectedTokens < targetThreshold`. If not, retries with higher compression level.

---

## Context Injection (How the Actor Sees Memories)

The `formatObservationsForContext()` function injects observations into the Actor's context with these instructions:

```
The following observations block contains your memory of past conversations with this user.

<observations>
[optimized observations with relative time annotations]
</observations>

IMPORTANT: When responding, reference specific details from these observations.
Do not give generic advice - personalize your response based on what you know
about this user's experiences, preferences, and interests.

KNOWLEDGE UPDATES: When asked about current state, always prefer the MOST RECENT
information. Observations include dates - if you see conflicting information,
the newer observation supersedes the older one. Look for phrases like "will start",
"is switching", "changed to", "moved to" as indicators.

PLANNED ACTIONS: If the user stated they planned to do something and the date
is now in the past, assume they completed the action unless there's evidence
they didn't.
```

### Optimization at Injection Time

`optimizeObservationsForContext()` strips:
1. Yellow and green priority emojis (keeps only red)
2. Semantic tags like `[label, label]`
3. Arrow indicators `->` replaced with space
4. Multiple spaces/newlines cleaned up

This means the Actor sees a cleaner, more compact version focused on high-priority items.

---

## Model Configuration

| Score | Answerer | Observer/Reflector |
|-------|----------|--------------------|
| 94.87% | gpt-5-mini | gemini-2.5-flash |
| 93.27% | gemini-3-pro | gemini-2.5-flash |
| 84.23% | gpt-4o | gemini-2.5-flash |

Note from the Mastra team: "Claude 4.5 models currently don't work well as observer or reflector."
Default Observer/Reflector model: `google/gemini-2.5-flash` (1M context window).

---

## Mastra RAG (80% -- Same Model, No OM)

Before building Observational Memory, Mastra hit 80% with GPT-4o using pure RAG. The techniques they discovered during this phase are instructive because they show what is achievable without architectural novelty:

**1. Date-Grouped Message Formatting (biggest single improvement)**
Group recalled messages by "Year, Month, Day" with time labels (e.g., "2:19 PM"). Clarify whether messages came from current or previous conversations. This single change dramatically improved temporal reasoning.

**2. Timestamp Correction**
Messages were initially timestamped with the benchmark run date instead of the original dataset dates. Fixing this --- using `question_date` from LongMemEval and adding "Today's date is `${question_date}`" to the system prompt --- brought the score from ~35% to 74%.

**3. Granular Memory Updates (vNext tool)**
An `updateReason` parameter distinguishing append-new-memory, clarify-existing-memory, and replace-irrelevant-memory. Optional `searchString` for targeted updates. Improved working memory to 57.34%.

**4. topK Scaling**
topK=2: 63.41%, topK=5: 73.98%, topK=10: 78.59%, topK=20: 80.05%. More context consistently helps up to topK=20.

---

## Why OM Works (Technical Analysis)

1. **No retrieval failures**: Everything is in context. Cannot miss relevant information.
2. **Compression not extraction**: 5-40x compression preserves distinguishing details that atomic fact extraction (our approach) loses at 50-100x.
3. **Temporal reasoning trivial**: LLM sees ALL dates in chronological order with relative annotations ("3 days ago", "likely already happened"). No date-range queries needed.
4. **State change tracking**: Observer explicitly notes superseded information with "replacing X" / "no longer at Y" phrasing. Actor prompt says "newer supersedes older."
5. **Prompt caching**: Context is append-only (except during reflection). Stable prefix = high cache hit rate = lower cost.
6. **Planned action inference**: If user said "I will do X on Monday" and Monday was 2 weeks ago, system annotates "likely already happened" -- helping with questions about completed activities.
7. **Detail preservation in lists**: Observer preserves distinguishing attributes for each item, not just counts. This directly helps enumeration questions.

---

## Limitations

- Bounded by context window size (~1M tokens for gemini-2.5-flash)
- For LongMemEval-S (~115K tokens raw), compresses to 3-30K -- fits easily
- For LongMemEval-M (~1.5M tokens), would need very aggressive compression or huge context
- Cost scales with context size (mitigated by prompt caching)
- Not a production architecture for unbounded conversation histories

---

## Relevance to Engram

Several Mastra techniques were directly adoptable without abandoning our Qdrant architecture:

**Adopted:**
- Date-grouped context formatting (implemented early, contributed +4pp)
- Timestamp correction (fixing `Utc::now()` was one of our first wins)
- "Knowledge Updates: prefer MOST RECENT" instructions in answering prompt

**Considered but not adopted:**
- Observer-style compression replacing atomic extraction (would require full re-ingestion and a fundamentally different storage model)
- Priority-based fact ranking during extraction
- Relative time annotations on retrieved facts ("3 days ago")

The key lesson from studying Mastra OM was not a specific technique but a strategic insight: **reasoning quality matters more than data architecture**. The top system on the leaderboard uses zero retrieval. The gap between systems is driven by how well the answering model reasons about the evidence it sees, not by how cleverly the evidence is organized in storage.
