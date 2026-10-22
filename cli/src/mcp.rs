//! `suture mcp`: the Model Context Protocol over stdin and stdout, so a
//! coding agent can call the loop of `suture help` and `suture check` as
//! tools (appendix AO.2.4). Messages are JSON-RPC 2.0, one per line.

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{json, Value};

/// What the agent is told when it connects.
const INSTRUCTIONS: &str = "Suture is a controlled-English language for backends: rules, permissions, parts and \
effects, tested by scenarios. Start with suture_help (topic `start`, then `recipes`), write the module's .st files and \
.scenario files in one directory, and call suture_check after every edit until it passes. suture_build gives the \
read-back of every rule, for the person to review.";

fn tools() -> Value {
    let dir = json!({ "type": "string", "description": "the module's directory: its .st files and .scenario files" });
    let mut topics: Vec<&str> = crate::help::topics().into_iter().map(|(t, _)| t).collect();
    topics.push("all");
    json!([
        {
            "name": "suture_help",
            "description": "A card of Suture's forms, idioms and limits, with examples that compile. Without a topic, the list of topics.",
            "inputSchema": { "type": "object", "properties": { "topic": { "type": "string", "enum": topics } } }
        },
        {
            "name": "suture_check",
            "description": "Compile the module and run every scenario beside it. Prints each error with file and line, ✓ per passing scenario, and the trace and failures of each failing one.",
            "inputSchema": { "type": "object", "properties": { "dir": dir }, "required": ["dir"] }
        },
        {
            "name": "suture_build",
            "description": "Compile the module, write its program file, and print the read-back: each rule in plain English and its cost, the choices, roles and what is admitted.",
            "inputSchema": { "type": "object", "properties": { "dir": dir }, "required": ["dir"] }
        },
        {
            "name": "suture_explain",
            "description": "Run a scenario file and explain one entity at the end of each scenario: its states, parts, totals and why a step waits.",
            "inputSchema": {
                "type": "object",
                "properties": { "dir": dir, "scenario": { "type": "string", "description": "the .scenario file" }, "name": { "type": "string", "description": "the entity's name in the scenario" } },
                "required": ["dir", "scenario", "name"]
            }
        }
    ])
}

/// A tool's output, and whether it is an error.
fn call(name: &str, args: &Value) -> (String, bool) {
    let arg = |k: &str| args.get(k).and_then(Value::as_str);
    let need = |k: &str| arg(k).ok_or_else(|| format!("`{k}` is missing"));
    let result = match name {
        "suture_help" => crate::help::help(arg("topic")).map(|t| (t, false)),
        "suture_check" => need("dir").map(|d| {
            let found = crate::check::check(Path::new(d));
            (found.text(), !found.passed())
        }),
        "suture_build" => need("dir").and_then(|d| crate::built(Path::new(d))).map(|t| (t, false)),
        "suture_explain" => need("dir").and_then(|d| {
            let (s, n) = (need("scenario")?, need("name")?);
            crate::scenarios_text(Path::new(d), Path::new(s), Some(n), false).map(|(t, ok)| (t, !ok))
        }),
        _ => Err(format!("no tool `{name}`")),
    };
    result.unwrap_or_else(|e| (e, true))
}

/// The answer to one message, or none for a notification.
pub fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "suture", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let (text, is_error) = call(name, params.get("arguments").unwrap_or(&Value::Null));
            json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
        }
        m => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("no method `{m}`") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

pub fn run() -> Result<bool, String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(a) = answer {
            writeln!(stdout, "{a}").and_then(|()| stdout.flush()).map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(method: &str, params: Value) -> Value {
        handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap()["result"].clone()
    }

    #[test]
    fn an_agent_lists_the_tools_reads_a_card_and_checks_a_module() {
        assert_eq!(ask("initialize", json!({ "protocolVersion": "2025-06-18" }))["serverInfo"]["name"], "suture");
        assert_eq!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })), None);
        let names: Vec<Value> = ask("tools/list", json!({}))["tools"].as_array().unwrap().iter().map(|t| t["name"].clone()).collect();
        assert_eq!(names, ["suture_help", "suture_check", "suture_build", "suture_explain"]);
        let card = ask("tools/call", json!({ "name": "suture_help", "arguments": { "topic": "recipes" } }));
        assert!(card["content"][0]["text"].as_str().unwrap().starts_with("# recipes:"));
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/returns");
        let checked = ask("tools/call", json!({ "name": "suture_check", "arguments": { "dir": dir } }));
        assert_eq!(checked["isError"], false, "{checked}");
        assert!(checked["content"][0]["text"].as_str().unwrap().ends_with("2 scenarios: 2 passed, 0 failed\n"));
        let missing = ask("tools/call", json!({ "name": "suture_check", "arguments": {} }));
        assert_eq!((missing["isError"].clone(), missing["content"][0]["text"].clone()), (json!(true), json!("`dir` is missing")));
        let unknown = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" })).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }
}
