//! Tests for tool schemas and date parsing.

#[cfg(test)]
mod tests {
    use crate::longmemeval::tools::date_parsing::parse_date_expression;
    use crate::longmemeval::tools::schemas::*;

    #[test]
    fn test_all_tool_schemas_valid_json() {
        let schemas = all_tool_schemas();
        assert_eq!(schemas.len(), 8);
        for schema in &schemas {
            assert!(schema["function"]["name"].is_string());
            assert!(schema["function"]["parameters"].is_object());
            assert_eq!(schema["type"].as_str().unwrap(), "function");
        }
    }

    #[test]
    fn test_tool_names() {
        let schemas = all_tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"search_facts"));
        assert!(names.contains(&"search_messages"));
        assert!(names.contains(&"grep_messages"));
        assert!(names.contains(&"get_session_context"));
        assert!(names.contains(&"get_by_date_range"));
        assert!(names.contains(&"search_entity"));
        assert!(names.contains(&"date_diff"));
        assert!(names.contains(&"done"));
    }

    #[test]
    fn test_search_facts_schema() {
        let schema = all_tool_schemas()
            .into_iter()
            .find(|s| s["function"]["name"] == "search_facts")
            .unwrap();
        let params = &schema["function"]["parameters"];
        assert!(params["properties"]["query"].is_object());
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("query")));
    }

    #[test]
    fn test_parse_exact_date() {
        let result = parse_date_expression("2023/06/15");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        // 2023-06-15 00:00:00 UTC
        assert_eq!(start.seconds, 1686787200);
        assert_eq!(end.seconds, 1686873599); // 23:59:59
    }

    #[test]
    fn test_parse_date_dash_format() {
        let result = parse_date_expression("2023-06-15");
        assert!(result.is_some());
        let (start, _end) = result.unwrap();
        assert_eq!(start.seconds, 1686787200);
    }

    #[test]
    fn test_parse_month_year() {
        let result = parse_date_expression("March 2023");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        // March 1, 2023 00:00:00
        assert_eq!(start.seconds, 1677628800);
        // March 31, 2023 23:59:59
        assert_eq!(end.seconds, 1680307199);
    }

    #[test]
    fn test_parse_year_only() {
        let result = parse_date_expression("2023");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        // Jan 1, 2023
        assert_eq!(start.seconds, 1672531200);
        // Dec 31, 2023 23:59:59
        assert_eq!(end.seconds, 1704067199);
    }

    #[test]
    fn test_parse_quarter() {
        let result = parse_date_expression("Q1 2023");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        // Jan 1, 2023
        assert_eq!(start.seconds, 1672531200);
        // March 31, 2023 23:59:59
        assert_eq!(end.seconds, 1680307199);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_date_expression("yesterday").is_none());
        assert!(parse_date_expression("").is_none());
        assert!(parse_date_expression("abc").is_none());
    }

    #[test]
    fn test_done_schema_has_answer() {
        let schema = done_schema();
        let params = &schema["function"]["parameters"];
        assert!(params["properties"]["answer"].is_object());
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("answer")));
    }

    #[test]
    fn test_tool_schemas_with_graph() {
        let schemas = tool_schemas(true);
        assert_eq!(schemas.len(), 12); // 8 base + 4 graph tools
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"graph_lookup"));
        assert!(names.contains(&"graph_relationships"));
        assert!(names.contains(&"graph_disambiguate"));
        assert!(names.contains(&"graph_enumerate"));
        assert!(names.contains(&"done"));
    }

    #[test]
    fn test_tool_schemas_without_graph() {
        let schemas = tool_schemas(false);
        assert_eq!(schemas.len(), 8); // no graph tools
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"graph_lookup"));
        assert!(!names.contains(&"graph_relationships"));
        assert!(!names.contains(&"graph_disambiguate"));
        assert!(!names.contains(&"graph_enumerate"));
    }
}
