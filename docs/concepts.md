---
title: Concepts
sidebar_position: 2
description: "The Engram memory model: fact types, epistemic types, entities, temporal versioning, and confidence scoring."
---

# Concepts

This document explains the memory model used by engram.

## What is a "memory"?

A memory is a structured fact extracted from a conversation. When you ingest a conversation, engram's LLM-based extractor identifies discrete, atomic facts and stores each one as an independent memory.

Example conversation:
> **User:** I work at Anthropic. My manager is Bob and we're in the safety team.

Extracted memories:
1. "User works at Anthropic" (State, entities: [user, anthropic])
2. "Bob is the user's manager" (Relation, entities: [user, bob])
3. "User is on the safety team at Anthropic" (State, entities: [user, anthropic])

Each memory is embedded as a vector and stored in Qdrant for semantic retrieval.

## Fact types

Every memory has a `fact_type` that classifies what kind of knowledge it represents:

| FactType | Description | Example |
|----------|-------------|---------|
| `State` | Current state of an entity | "Alice works at Google" |
| `Event` | Something that happened | "Alice got promoted in March" |
| `Preference` | Subjective preference | "Alice prefers Python over Java" |
| `Relation` | Relationship between entities | "Bob is Alice's manager" |

## Epistemic types

The `epistemic_type` determines which Qdrant collection the memory is stored in:

| EpistemicType | Collection | Description |
|---------------|------------|-------------|
| `World` | `world` | Objective external facts |
| `Experience` | `experience` | First-person agent experiences |
| `Opinion` | `opinion` | Subjective beliefs with confidence |
| `Observation` | `observation` | Preference-neutral observations |

Search queries fan out across all four collections and results are merged.

## Source types

`source_type` records how confident we are in the fact based on how it was learned:

| SourceType | Confidence multiplier | Description |
|------------|----------------------|-------------|
| `UserExplicit` | 1.0 | User directly stated the fact |
| `UserImplied` | 0.8 | Strongly implied by user's words |
| `Derived` | 0.6 | Inferred from other memories |
| `AssistantStated` | 0.3 | AI mentioned it, user didn't confirm |

## Entities

Entities are people, organizations, locations, topics, or events mentioned in memories. Each entity has:

- **name**: Display name ("Anthropic")
- **normalized_id**: Lowercase identifier for matching ("anthropic")
- **entity_type**: One of `Person`, `Organization`, `Location`, `Topic`, `Event`

Entities are stored as payload fields on the Qdrant point, enabling filtered search (e.g., "find all memories about Anthropic").

## Temporal model

Engram uses a bi-temporal model:

| Field | Description |
|-------|-------------|
| `t_created` | When the memory was stored in engram |
| `t_valid` | When the fact became true in the real world |
| `t_expired` | When the fact stopped being true (set on supersede/delete) |

This enables queries like "what was true as of January 2025?" by filtering on `t_valid`.

## Memory versioning

Facts change over time. Engram handles updates through versioning:

- **`supersedes_id`**: Points to the older memory this one replaces
- **`derived_from_ids`**: IDs of memories this was synthesized from
- **`is_latest`**: Whether this is the current version of the fact

When a new conversation contradicts an existing fact (e.g., "I changed jobs"), the extractor creates a new memory that supersedes the old one. The old memory's `t_expired` is set and `is_latest` becomes `false`.

Search results only include `is_latest = true` memories by default.

## Confidence scoring

Each memory has a `confidence` score in [0.0, 1.0]:

- Base confidence comes from the extraction model's assessment
- Multiplied by the `source_type` confidence multiplier
- Used for filtering (`min_confidence` in search) and ranking

## Abstention

When a search query finds no relevant results (or results below the confidence threshold), engram can **abstain** rather than returning low-quality results. The response will include:

```json
{
  "abstained": true,
  "abstention_reason": "No relevant memories found for the query",
  "memories": [],
  "total_found": 0
}
```

The abstention threshold is configurable via `retrieval.abstention_threshold` in `engram.toml`.

## Collections

Engram uses five Qdrant collections:

| Collection | Contents |
|------------|----------|
| `world` | World-knowledge facts (EpistemicType::World) |
| `experience` | First-person experiences (EpistemicType::Experience) |
| `opinion` | Opinions and beliefs (EpistemicType::Opinion) |
| `observation` | Neutral observations (EpistemicType::Observation) |
| `messages` | Raw conversation messages (used internally) |

The first four store extracted facts. The `messages` collection stores raw conversation turns for message-level search via `POST /v1/messages/search`.

**Note:** The current ingestion pipeline (`POST /v1/memories`) stores extracted facts only. Raw messages are not automatically stored in the `messages` collection — see [Limitations](limitations) for details.
