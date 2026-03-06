//! Tool schema definitions for the memory agent.
//!
//! JSON schemas for OpenAI function-calling format. These define the 8 production
//! tools: search_facts, search_messages, grep_messages, get_session_context,
//! get_by_date_range, search_entity, date_diff, done.

use serde_json::{json, Value};

/// All production tool schemas (without graph tools).
pub fn tool_schemas() -> Vec<Value> {
    vec![
        search_facts_schema(),
        search_messages_schema(),
        grep_messages_schema(),
        get_session_context_schema(),
        get_by_date_range_schema(),
        search_entity_schema(),
        date_diff_schema(),
        done_schema(),
    ]
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

/// Schema for the done() tool that signals completion with a final answer.
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
