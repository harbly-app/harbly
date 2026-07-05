//! The shared library tool surface. One definition feeds three consumers:
//! the Anthropic tool-use loop, the OpenAI function-calling loop, and the MCP
//! server's tools/list (same names, same schemas — a model moved between
//! supplies sees the identical world).

use serde_json::{json, Value};

pub const SEARCH: &str = "search_library";
pub const LIST: &str = "list_assets";
pub const READ: &str = "read_asset";
pub const WRITE: &str = "write_asset";
pub const CREATE: &str = "create_asset";
pub const DELETE: &str = "delete_asset";

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: SEARCH,
            description: "Full-text search over the user's asset library (titles, body text and \
                          tags; Chinese word segmentation included). Returns matching assets \
                          with their asset_id.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keywords" }
                },
                "required": ["query"]
            }),
        },
        ToolSpec {
            name: LIST,
            description: "List assets with file name, folder, title, size in bytes and version \
                          count (capped at 300, newest first for the whole library). Use this to \
                          enumerate files — including empty ones full-text search cannot find.",
            schema: json!({
                "type": "object",
                "properties": {
                    "folder": { "type": "string", "description": "Library-relative folder to list; empty or omitted = the whole library" }
                },
                "required": []
            }),
        },
        ToolSpec {
            name: READ,
            description: "Read the current content of one asset by asset_id.",
            schema: json!({
                "type": "object",
                "properties": {
                    "asset_id": { "type": "string" }
                },
                "required": ["asset_id"]
            }),
        },
        ToolSpec {
            name: WRITE,
            description: "Replace an asset's content with the COMPLETE new file. The write is \
                          recorded as a new version in the asset's history — the user can \
                          inspect the diff and roll back at any time.",
            schema: json!({
                "type": "object",
                "properties": {
                    "asset_id": { "type": "string" },
                    "content": { "type": "string", "description": "The complete new file content" },
                    "summary": { "type": "string", "description": "One-line summary of the change" }
                },
                "required": ["asset_id", "content"]
            }),
        },
        ToolSpec {
            name: CREATE,
            description: "Create a new asset in the library from the given content. Use a file \
                          name with .html or .md extension (defaults to .html).",
            schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "File name, e.g. merged.html" },
                    "content": { "type": "string", "description": "The complete file content" },
                    "folder": { "type": "string", "description": "Library-relative folder; empty = root" }
                },
                "required": ["name", "content"]
            }),
        },
        ToolSpec {
            name: DELETE,
            description: "Move an asset to the system Trash (the user can restore it from \
                          Finder). Confirm the target via list_assets/read_asset first; never \
                          guess ids.",
            schema: json!({
                "type": "object",
                "properties": {
                    "asset_id": { "type": "string" }
                },
                "required": ["asset_id"]
            }),
        },
    ]
}

/// Human-readable one-liner for a tool call, shown as an activity row in the
/// panel. Strips the MCP prefix so agent and BYOK calls read the same.
pub fn call_label(name: &str, args: &Value) -> String {
    let name = name.strip_prefix("mcp__harbly__").unwrap_or(name);
    let detail = match name {
        SEARCH => args["query"]
            .as_str()
            .map(|q| format!("\u{201c}{q}\u{201d}")),
        LIST => args["folder"]
            .as_str()
            .filter(|f| !f.is_empty())
            .map(String::from),
        READ | WRITE | DELETE => args["asset_id"].as_str().map(short_id),
        CREATE => args["name"].as_str().map(String::from),
        _ => None,
    };
    match detail {
        Some(d) => format!("{name} {d}"),
        None => name.to_string(),
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_complete_and_valid_schemas() {
        let specs = tool_specs();
        assert_eq!(specs.len(), 6);
        for s in &specs {
            assert_eq!(s.schema["type"], "object");
            assert!(s.schema["required"].is_array());
            assert!(!s.description.is_empty());
        }
    }

    #[test]
    fn labels_cover_both_naming_styles() {
        let l = call_label("mcp__harbly__search_library", &json!({"query": "定价"}));
        assert_eq!(l, "search_library “定价”");
        let l = call_label(WRITE, &json!({"asset_id": "abcd1234-rest", "content": "x"}));
        assert_eq!(l, "write_asset abcd1234");
        let l = call_label(CREATE, &json!({"name": "merged.html", "content": "x"}));
        assert_eq!(l, "create_asset merged.html");
        assert_eq!(call_label("unknown_tool", &json!({})), "unknown_tool");
    }
}
