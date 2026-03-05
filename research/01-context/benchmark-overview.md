---
title: "The LongMemEval-S Benchmark"
sidebar_position: 1
---

# The LongMemEval-S Benchmark

LongMemEval-S is the "small" variant of the LongMemEval benchmark, introduced by Di Wu et al. in their ICLR 2025 paper ([arxiv.org/abs/2410.10813](https://arxiv.org/abs/2410.10813)). It has become the standard evaluation for production-grade conversational memory systems, and it was the benchmark we chose to drive Engram's development from its first commit.

## Structure

The benchmark consists of **500 questions** drawn from approximately **24,000 synthetic conversation sessions** between simulated users and an AI assistant. Each session represents a natural multi-turn conversation covering topics like travel planning, work projects, personal preferences, and daily life. The questions test whether a memory system can correctly recall, reason about, and synthesize information buried across these conversations.

The 500 questions are divided into five categories, each targeting a distinct memory capability:

### Extraction (150 questions)

The most straightforward category. These questions ask about facts that were explicitly stated in a single conversation session --- a user's preference, a specific detail about their life, something the assistant recommended. The challenge is not reasoning but *retrieval coverage*: can the system find the right fact among tens of thousands of stored memories?

Extraction is further divided into three sub-categories:
- **SS-User** (70 questions): Facts stated by the user ("I have three kids")
- **SS-Assistant** (56 questions): Facts stated by the assistant ("I recommended Hotel Marais")
- **SS-Preference** (30 questions): User preferences ("I prefer direct, no-nonsense communication")

### Knowledge Updates (72 questions)

These questions test whether the system correctly tracks *changes* over time. A user might mention they are learning Python in one session and later say they switched to Rust. The system must return the most recent value, not the stale one. This requires temporal ordering of extracted facts and explicit handling of superseded information.

### Temporal (127 questions)

The largest and most challenging category. Questions require reasoning about time: "When was the last time the user mentioned working from home?", "How many days between the user's trip to Paris and their move to a new apartment?", "What was the user doing three weeks before their birthday?" These demand not just storing timestamps but performing temporal arithmetic and anchoring relative time references.

### Multi-Session (121 questions)

These questions require synthesizing information scattered across multiple conversation sessions. "How many different cities has the user traveled to?", "List all the books the user has mentioned reading." The system must aggregate facts from many sessions, deduplicate overlapping mentions, and produce a complete enumeration. This is where simple single-query retrieval breaks down --- finding *all* relevant facts requires either exhaustive search or iterative refinement.

### Abstention (30 questions)

The smallest but in some ways the trickiest category. These questions ask about things the user *never mentioned*. The correct answer is "I don't have enough information" or equivalent. The system must resist the temptation to hallucinate an answer from partially matching facts. Entity conflation is the primary failure mode: the system finds facts about a similar-sounding person or topic and confabulates an answer.

## Key Findings from the Paper

The original LongMemEval paper evaluated several baseline memory architectures and published findings that directly informed our approach. Three results stood out:

### 1. Facts as KEY, not VALUE

The paper found that replacing stored conversation content with extracted atomic facts --- using facts as the *value* in the retrieval index --- **hurts accuracy**. Information needed to answer questions is lost during extraction. However, using facts to *augment the retrieval index* as keys, while preserving raw conversation turns as values, improves both recall (+9.4%) and accuracy (+5.4%).

The optimal approach is: **facts as keys, raw turns as values**.

This finding was directly relevant to Engram, because our initial architecture stored only extracted atomic facts and discarded raw conversation turns entirely. The paper's analysis suggested this was the worst of the three approaches (raw only, facts only, facts+raw). We ultimately chose to optimize our fact extraction pipeline rather than re-architect to preserve raw turns --- a pragmatic trade-off documented in the [Architecture section](../architecture/).

### 2. Time-aware query expansion

Extracting temporal ranges from queries and filtering irrelevant content improved recall by +6.8% to +11.3% on temporal questions. This motivated our investment in temporal handling throughout the pipeline: timestamp correction (replacing erroneous `Utc::now()` with actual session dates), date-grouped context formatting, and time-aware query rewriting.

### 3. Chain-of-Note prompting

Generating reading notes for each retrieved document before answering improved accuracy by up to +10 absolute points. This style of deliberative reasoning over retrieved context influenced our agent-loop answering design, where the model iteratively examines evidence before committing to an answer.

## Benchmark Mechanics

Each question in LongMemEval-S is associated with:
- A **question text** and a **question date** (the simulated "today" when the question is asked)
- A **ground truth answer** (the expected correct response)
- A **user ID** linking to the subset of conversation sessions belonging to that user
- A **category label** (Extraction, Updates, Temporal, MultiSession, Abstention)

Systems are evaluated by an LLM judge that compares the system's answer to the ground truth. The judge assigns a binary correct/incorrect verdict per question. The overall score is simply the count of correct answers out of 500 (reported as a percentage or raw count).

There is also a larger variant, LongMemEval-M, with approximately 1.5M tokens of raw conversation data. We focused exclusively on LongMemEval-S because it is the variant used by all leaderboard entries, enabling direct comparison.
