//! Tool schema definitions for the agentic answering loop.

use serde_json::{json, Value};

/// All tool schemas for the agentic answering loop
pub fn all_tool_schemas() -> Vec<Value> {
    tool_schemas(false)
}

/// Tool schemas with optional graph retrieval
pub fn tool_schemas(include_graph: bool) -> Vec<Value> {
    let mut schemas = vec![
        search_facts_schema(),
        search_messages_schema(),
        grep_messages_schema(),
        get_session_context_schema(),
        get_by_date_range_schema(),
        search_entity_schema(),
    ];
    if include_graph {
        schemas.push(graph_lookup_schema());
        schemas.push(graph_relationships_schema());
        schemas.push(graph_disambiguate_schema());
        schemas.push(graph_enumerate_schema());
    }
    schemas.push(date_diff_schema());
    schemas.push(done_schema());
    schemas
}

fn search_facts_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "search_facts",
            "description": "Semantic search over extracted facts from conversations. Returns facts with dates and session IDs. Use for finding specific information about the user.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query. Be specific — include names, topics, or keywords."
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Number of results to return. Default 10.",
                        "default": 10
                    },
                    "level": {
                        "type": "string",
                        "description": "Optional filter: 'explicit' for direct statements, 'deductive' for inferred facts, 'contradiction' for updated info.",
                        "enum": ["explicit", "deductive", "contradiction"]
                    }
                },
                "required": ["query"]
            }
        }
    })
}

fn search_messages_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "search_messages",
            "description": "Semantic search over raw conversation messages. Returns actual user/assistant turns with dates. Use when facts don't have enough detail.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query."
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Number of results to return. Default 10.",
                        "default": 10
                    }
                },
                "required": ["query"]
            }
        }
    })
}

fn grep_messages_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "grep_messages",
            "description": "Exact text search over raw messages. Use for finding specific names, numbers, or phrases that semantic search might miss.",
            "parameters": {
                "type": "object",
                "properties": {
                    "substring": {
                        "type": "string",
                        "description": "The exact text to search for (case-insensitive)."
                    }
                },
                "required": ["substring"]
            }
        }
    })
}

fn get_session_context_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_session_context",
            "description": "Get surrounding conversation turns from a specific session. Use to expand context around a fact or message you found. Optionally include extracted facts from the same session.",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The session ID to retrieve from."
                    },
                    "turn_index": {
                        "type": "integer",
                        "description": "The turn index to center around. Default 0 (start of session).",
                        "default": 0
                    },
                    "window": {
                        "type": "integer",
                        "description": "Number of turns before and after to include. Default 5.",
                        "default": 5
                    },
                    "include_facts": {
                        "type": "boolean",
                        "description": "If true, also return extracted facts from this session (max 10). Default false.",
                        "default": false
                    }
                },
                "required": ["session_id"]
            }
        }
    })
}

fn get_by_date_range_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_by_date_range",
            "description": "Get messages within a date range. Supports natural language dates. Use for temporal questions like 'what happened in March 2023'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "start_date": {
                        "type": "string",
                        "description": "Start date. Formats: YYYY/MM/DD, YYYY-MM-DD, 'March 2023', '2023', 'Q1 2023'."
                    },
                    "end_date": {
                        "type": "string",
                        "description": "End date. Same formats as start_date."
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional text search within the date range."
                    }
                },
                "required": ["start_date", "end_date"]
            }
        }
    })
}

fn search_entity_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "search_entity",
            "description": "Find all facts and messages mentioning a specific entity (person, place, org). Searches the entity_ids field in facts and content in messages.",
            "parameters": {
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "The entity name to search for."
                    }
                },
                "required": ["entity"]
            }
        }
    })
}

fn graph_lookup_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "graph_lookup",
            "description": "Look up an entity in the knowledge graph. Returns the entity profile including type, aliases, relationships, and associated facts. If multiple entities share the same name, returns all matches with distinguishing info.",
            "parameters": {
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "The entity name to look up."
                    }
                },
                "required": ["entity"]
            }
        }
    })
}

fn graph_relationships_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "graph_relationships",
            "description": "Find entities by relationship type. Query the knowledge graph for specific relationship patterns like 'who works for Acme Corp?' or 'all relationships for Alice'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "The entity name to find relationships for."
                    },
                    "relation": {
                        "type": "string",
                        "description": "Relationship type filter: 'owned_by', 'located_in', 'works_for', 'part_of', 'related_to', 'associated_with', or 'all' for all types."
                    }
                },
                "required": ["entity"]
            }
        }
    })
}

fn graph_disambiguate_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "graph_disambiguate",
            "description": "Disambiguate entities with the same name using context clues. When multiple people/places/things share a name, provide context keywords to identify the right one.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The ambiguous entity name to disambiguate."
                    },
                    "context": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Context keywords to help identify the right entity (e.g., ['yoga', 'Tuesday class'])."
                    }
                },
                "required": ["name"]
            }
        }
    })
}

fn graph_enumerate_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "graph_enumerate",
            "description": "List all entities of a given type from the knowledge graph. Returns total count and entity names with mention frequency. Prefer this for count/list questions when the answer maps to an entity type. Available types: person, organization, location, product, datetime, other.",
            "parameters": {
                "type": "object",
                "properties": {
                    "entity_type": {
                        "type": "string",
                        "description": "Entity type to list.",
                        "enum": ["person", "organization", "location", "product", "datetime", "other"]
                    },
                    "keyword": {
                        "type": "string",
                        "description": "(optional) Filter entities whose name contains this keyword (case-insensitive)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max entities to return (default 30, max 100)."
                    }
                },
                "required": ["entity_type"]
            }
        }
    })
}

fn date_diff_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "date_diff",
            "description": "Compute the exact difference between two dates. Use this for ANY question asking 'how many days/weeks/months' between events. Do NOT do date arithmetic in your head — always use this tool.",
            "parameters": {
                "type": "object",
                "properties": {
                    "start_date": {
                        "type": "string",
                        "description": "The earlier date in YYYY/MM/DD or YYYY-MM-DD format."
                    },
                    "end_date": {
                        "type": "string",
                        "description": "The later date in YYYY/MM/DD or YYYY-MM-DD format."
                    },
                    "unit": {
                        "type": "string",
                        "description": "Unit for the result: 'days', 'weeks', 'months', or 'years'. Default 'days'.",
                        "enum": ["days", "weeks", "months", "years"]
                    },
                    "inclusive": {
                        "type": "boolean",
                        "description": "If true, include both start and end date in count. Default false.",
                        "default": false
                    }
                },
                "required": ["start_date", "end_date"]
            }
        }
    })
}

pub fn done_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "done",
            "description": "Signal that you have enough information to answer. Call this with your final answer.",
            "parameters": {
                "type": "object",
                "properties": {
                    "answer": {
                        "type": "string",
                        "description": "Your final answer to the question. Be concise — a name, number, date, or short phrase."
                    },
                    "latest_date": {
                        "type": "string",
                        "description": "For update questions: YYYY/MM/DD of the most recent evidence you found."
                    },
                    "computed_value": {
                        "type": "string",
                        "description": "For temporal computation questions: the numeric result from date_diff."
                    }
                },
                "required": ["answer"]
            }
        }
    })
}
