//! Read-path normalization of `outputSchema` on `tools/list` entries (#531).
//!
//! MCP requires `outputSchema`, when present, to be a JSON Schema object with a
//! top-level `"type": "object"`. Some upstream servers instead emit a
//! discriminated union (top-level `oneOf` with no `type`), and strict clients
//! validate the whole `tools/list` result, so a single offending entry makes the
//! entire connection fail. The gateway therefore repairs or drops the field
//! before the response leaves.
//!
//! Per tool entry:
//! - no `outputSchema`, or top-level `"type": "object"` -> untouched;
//! - `type` missing but an object keyword (`properties`, `oneOf`, `anyOf`,
//!   `allOf`, `required`) is present -> add `"type": "object"`, which is
//!   semantics preserving because every branch behind those keywords is an
//!   object;
//! - `type` is an array containing `"object"` -> collapse it to `"object"`;
//! - anything else (non-object schema, incompatible `type`, or a `type`-less
//!   schema without an object keyword) -> drop `outputSchema`, which MCP treats
//!   as optional.
//!
//! Only read paths call this; `McpCatalogCache` snapshots keep the raw upstream
//! bytes for the admin catalog and troubleshooting.

use serde_json::Value;

const OBJECT_TYPE: &str = "object";

/// Top-level JSON Schema keywords that imply an object when they appear without
/// a `type` discriminator.
const OBJECT_KEYWORDS: [&str; 5] = ["properties", "oneOf", "anyOf", "allOf", "required"];

/// Decision for one `outputSchema` value.
enum SchemaFix {
    Keep,
    EnsureObjectType,
    Drop,
}

pub(super) fn normalize_tools_list_result(response: &mut Value) {
    let Some(items) = response
        .pointer_mut("/result/tools")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for item in items {
        normalize_tool_output_schema(item);
    }
}

pub(super) fn normalize_tool_output_schema(tool: &mut Value) {
    let Some(tool) = tool.as_object_mut() else {
        return;
    };
    let fix = match tool.get("outputSchema") {
        None => return,
        Some(schema) => schema_fix(schema),
    };
    match fix {
        SchemaFix::Keep => {}
        SchemaFix::EnsureObjectType => {
            if let Some(Value::Object(schema)) = tool.get_mut("outputSchema") {
                schema.insert("type".to_string(), Value::String(OBJECT_TYPE.to_string()));
            }
        }
        SchemaFix::Drop => {
            tool.remove("outputSchema");
        }
    }
}

fn schema_fix(schema: &Value) -> SchemaFix {
    let Value::Object(schema) = schema else {
        return SchemaFix::Drop;
    };
    match schema.get("type") {
        Some(Value::String(kind)) if kind == OBJECT_TYPE => SchemaFix::Keep,
        Some(Value::Array(kinds)) => {
            if kinds.iter().any(|kind| kind.as_str() == Some(OBJECT_TYPE)) {
                SchemaFix::EnsureObjectType
            } else {
                SchemaFix::Drop
            }
        }
        Some(_) => SchemaFix::Drop,
        None => {
            if schema
                .keys()
                .any(|key| OBJECT_KEYWORDS.contains(&key.as_str()))
            {
                SchemaFix::EnsureObjectType
            } else {
                SchemaFix::Drop
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    fn tool_with(schema: Value) -> Value {
        json!({"name": "t", "description": "d", "inputSchema": {"type": "object"}, "outputSchema": schema})
    }

    #[test]
    fn adds_object_type_when_missing_with_object_keyword() {
        for keyword in OBJECT_KEYWORDS {
            let mut schema = Map::new();
            schema.insert(keyword.to_string(), json!([]));
            let mut tool = tool_with(Value::Object(schema));

            normalize_tool_output_schema(&mut tool);

            assert_eq!(
                tool["outputSchema"]["type"], OBJECT_TYPE,
                "keyword {keyword}"
            );
            assert!(
                tool["outputSchema"].get(keyword).is_some(),
                "keyword {keyword}"
            );
        }
    }

    #[test]
    fn keeps_schema_with_object_type() {
        let mut tool = tool_with(json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "required": ["a"],
        }));
        let expected = tool.clone();

        normalize_tool_output_schema(&mut tool);

        assert_eq!(tool, expected);
    }

    #[test]
    fn collapses_type_array_containing_object() {
        let mut tool = tool_with(json!({"type": ["object", "null"], "properties": {}}));

        normalize_tool_output_schema(&mut tool);

        assert_eq!(tool["outputSchema"]["type"], OBJECT_TYPE);
        assert!(tool["outputSchema"]["properties"].is_object());
    }

    #[test]
    fn drops_unfixable_schemas() {
        for schema in [
            json!("object"),
            json!(["object"]),
            json!(4),
            json!(null),
            json!({"type": "string"}),
            json!({"type": ["string", "number"]}),
            json!({"description": "no discriminator"}),
        ] {
            let mut tool = tool_with(schema.clone());

            normalize_tool_output_schema(&mut tool);

            assert!(tool.get("outputSchema").is_none(), "schema {schema}");
        }
    }

    #[test]
    fn leaves_tools_without_output_schema_and_other_fields_untouched() {
        let mut tool = json!({"name": "t", "inputSchema": {"type": "object"}});
        let expected = tool.clone();
        normalize_tool_output_schema(&mut tool);
        assert_eq!(tool, expected);

        let mut tool = tool_with(json!({"oneOf": [{"type": "object"}]}));
        tool["title"] = json!("kept");
        normalize_tool_output_schema(&mut tool);
        assert_eq!(tool["name"], "t");
        assert_eq!(tool["description"], "d");
        assert_eq!(tool["title"], "kept");
        assert_eq!(tool["inputSchema"], json!({"type": "object"}));
    }

    #[test]
    fn normalizes_every_entry_of_tools_list_result() {
        let mut response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    {"name": "a", "outputSchema": {"oneOf": [{"type": "object"}]}},
                    {"name": "b", "outputSchema": {"type": "object"}},
                    {"name": "c", "outputSchema": {"type": "string"}},
                ]
            }
        });

        normalize_tools_list_result(&mut response);

        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools[0]["outputSchema"]["type"], OBJECT_TYPE);
        assert_eq!(
            tools[1],
            json!({"name": "b", "outputSchema": {"type": "object"}})
        );
        assert!(tools[2].get("outputSchema").is_none());
        assert_eq!(tools[2]["name"], "c");
    }

    #[test]
    fn ignores_responses_without_tools_array() {
        for mut response in [
            json!({"jsonrpc": "2.0", "id": 1}),
            json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": "nope"}}),
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32601}}),
        ] {
            let expected = response.clone();
            normalize_tools_list_result(&mut response);
            assert_eq!(response, expected);
        }
    }

    #[test]
    fn ignores_non_object_tool_entries() {
        let mut response = json!({"result": {"tools": ["not-a-tool", {"name": "a"}]}});
        let expected = response.clone();
        normalize_tools_list_result(&mut response);
        assert_eq!(response, expected);
    }
}
