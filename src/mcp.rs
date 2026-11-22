//! `leadstinger-cli mcp`: a thin MCP gateway on stdio (docs/CLI.md "MCP").
//!
//! Four tools in front of the whole registry, so an MCP client pays for four tool
//! definitions, not 195: `commands` (find a command and its parameters), `query` (run a
//! read-only command), `run` (run one command, or several as one `batch.run`) and
//! `screenshot` (the window as an image). Programs that can run code should use the CLI or
//! the Python client instead; this is for clients that have no shell.
//!
//! The protocol is the small part of MCP a tools-only stdio server needs (JSON-RPC 2.0,
//! one message per line): `initialize`, `ping`, `tools/list`, `tools/call`, and
//! notifications, which need no answer. A [`Backend`] runs the commands; the CLI's reaches
//! the running app or the library for each call.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::commands::{spec, Kind, COMMANDS};
use crate::controller::CmdError;

/// The protocol versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "LeadStinger is a live soundboard: sets of sound tiles that play on a hotkey, a click or a cue list. \
Every action in the app is a registry command. Find one with `commands` (a word, or an id for its parameters), \
read state with `query` (`status` first: what's playing, the current set), act with `run`, and look with `screenshot`. \
Tiles are named by title, `Set/Title`, `key:q`, `#3` or id; an ambiguous name fails with its candidates, so pick one, never guess. \
For many changes, send them as one `run` with `steps`: one undo step, nothing kept if a step fails; add `dryRun: true` to see the changes first.";

/// What the tools reach: the CLI's backend opens the app or the library for each call.
pub trait Backend {
    /// Run one registry command (or `batch.run`) with the caller's policy.
    fn call(&mut self, command: &str, params: Value) -> Result<Value, CmdError>;
    /// A PNG of the window (`view.screenshot`, waited for).
    fn screenshot(&mut self, params: Value) -> Result<Vec<u8>, CmdError>;
}

pub struct Server<B: Backend> {
    backend: B,
    /// `--allow-destructive`: deleting sets and tiles, and quitting the app, are offered.
    allow_destructive: bool,
}

impl<B: Backend> Server<B> {
    pub fn new(backend: B, allow_destructive: bool) -> Self {
        Server { backend, allow_destructive }
    }

    /// Serve until stdin closes.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.message(&line) {
                writeln!(output, "{reply}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One JSON-RPC message; `None` for a notification (or a reply), which gets no answer.
    pub fn message(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(rpc_error(Value::Null, -32700, &format!("not JSON: {e}"))),
        };
        let id = msg.get("id").cloned()?;
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return None; // a reply to something we never ask
        };
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or(VERSIONS[0]);
                let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": crate::brand::SLUG, "title": crate::brand::APP_NAME, "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.tools()})),
            "tools/call" => match params["name"].as_str() {
                Some(name) if TOOLS.contains(&name) => Ok(self.call_tool(name, params.get("arguments").cloned().unwrap_or_else(|| json!({})))),
                Some(name) => Err((-32602, format!("no tool {name:?}"))),
                None => Err((-32602, "tools/call needs a name".to_owned())),
            },
            _ => Err((-32601, format!("no method {method:?}"))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, m)) => rpc_error(id, code, &m),
        })
    }

    fn tools(&self) -> Value {
        let ann = |title: &str, read_only: bool, destructive: bool| {
            json!({"title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "openWorldHint": false})
        };
        let step = json!({"type": "object", "properties": {
            "command": {"type": "string", "description": "A command id, e.g. tile.set"},
            "params": {"type": "object", "description": "Its parameters, as `commands` shows them"},
        }, "required": ["command"]});
        json!([
            {
                "name": "commands",
                "description": "Find LeadStinger commands. With `filter`, the commands whose id or label has those words (id, label, kind: query = read-only, library = changes sets/tiles/settings, live = needs the running app). With `id`, that command's parameters in full: its summary and a JSON Schema.",
                "inputSchema": {"type": "object", "properties": {
                    "filter": {"type": "string", "description": "Words to look for, e.g. \"fade\" or \"tile.\""},
                    "id": {"type": "string", "description": "One command's id, for its parameters"},
                }},
                "annotations": ann("Find commands", true, false),
            },
            {
                "name": "query",
                "description": "Run a read-only command (kind query) and return its JSON: `status` (what's playing, the current set, the master level), `tile.list`, `tile.get`, `set.list`, `library.get` and the rest.",
                "inputSchema": {"type": "object", "properties": {
                    "command": {"type": "string", "description": "A query command's id, e.g. status"},
                    "params": {"type": "object", "description": "Its parameters"},
                }, "required": ["command"]},
                "annotations": ann("Read state", true, false),
            },
            {
                "name": "run",
                "description": if self.allow_destructive {
                    "Run one command (`command`, `params`), or several as one batch (`steps`): a batch runs with nothing else acting in between and is one undo step; when a step fails, its changes to sets and tiles are put back unless `atomic` is false. `dryRun: true` reports what a batch would change and changes nothing. Commands that start a job (an export, a transcription) return its id at once: ask `query` render.status or transcribe.status. Deleting sets and tiles is allowed on this server."
                } else {
                    "Run one command (`command`, `params`), or several as one batch (`steps`): a batch runs with nothing else acting in between and is one undo step; when a step fails, its changes to sets and tiles are put back unless `atomic` is false. `dryRun: true` reports what a batch would change and changes nothing. Commands that start a job (an export, a transcription) return its id at once: ask `query` render.status or transcribe.status. Deleting sets and tiles and quitting the app are refused here; ask the user to do those."
                },
                "inputSchema": {"type": "object", "properties": {
                    "command": {"type": "string", "description": "One command's id, e.g. tile.trigger"},
                    "params": {"type": "object", "description": "Its parameters, e.g. {\"tile\": \"Rimshot\"}"},
                    "steps": {"type": "array", "items": step, "description": "Several commands, run as one batch instead of `command`"},
                    "atomic": {"type": "boolean", "description": "Batch: put everything back when a step fails (default true)"},
                    "dryRun": {"type": "boolean", "description": "Batch: report the changes and change nothing (default false)"},
                }},
                "annotations": ann("Run commands", false, self.allow_destructive),
            },
            {
                "name": "screenshot",
                "description": "The LeadStinger window as a PNG image, to see what the user sees. Needs the app running with its window.",
                "inputSchema": {"type": "object", "properties": {
                    "window": {"type": "string", "description": "A set open in a window of its own: that window instead of the main one"},
                }},
                "annotations": ann("Look at the window", true, false),
            },
        ])
    }

    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        let result = match name {
            "commands" => Ok(commands(&args)),
            "query" => self.query(&args),
            "run" => self.run(&args),
            "screenshot" => match self.backend.screenshot(args) {
                Ok(png) => return json!({"content": [{"type": "image", "data": base64(&png), "mimeType": "image/png"}]}),
                Err(e) => Err(e),
            },
            _ => unreachable!("checked against TOOLS"),
        };
        match result {
            Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}], "structuredContent": wrap(v)}),
            Err(e) => json!({"content": [{"type": "text", "text": crate::control::error_json(&e).to_string()}], "isError": true}),
        }
    }

    fn query(&mut self, args: &Value) -> Result<Value, CmdError> {
        let id = args["command"].as_str().ok_or_else(|| bad("query needs a command"))?;
        match spec(id) {
            Some(s) if s.kind == Kind::Query => self.backend.call(id, params(args)),
            Some(_) => Err(bad(&format!("{id} isn't read-only: use run"))),
            None => Err(CmdError { code: "unknown_command", message: format!("no command {id:?}: find one with commands"), candidates: vec![] }),
        }
    }

    fn run(&mut self, args: &Value) -> Result<Value, CmdError> {
        let (id, params) = match (args["command"].as_str(), args.get("steps")) {
            (Some(id), None) => (id.to_owned(), params(args)),
            (None, Some(steps)) => {
                let mut p = json!({"steps": steps});
                for k in ["atomic", "dryRun"] {
                    if let Some(v) = args.get(k) {
                        p[k] = v.clone();
                    }
                }
                ("batch.run".to_owned(), p)
            }
            _ => return Err(bad("run takes a command (with params), or steps")),
        };
        if !self.allow_destructive {
            let ids: Vec<&str> = if id == "batch.run" {
                params["steps"].as_array().into_iter().flatten().filter_map(|s| s["command"].as_str()).collect()
            } else {
                vec![id.as_str()]
            };
            if let Some(d) = ids.into_iter().find(|i| spec(i).is_some_and(|s| s.destructive) || *i == "app.quit") {
                return Err(CmdError {
                    code: "refused",
                    message: format!("{d} isn't offered here: ask the user to do it in the app (or start this server with --allow-destructive)"),
                    candidates: vec![],
                });
            }
        }
        self.backend.call(&id, params)
    }
}

const TOOLS: &[&str] = &["commands", "query", "run", "screenshot"];

fn params(args: &Value) -> Value {
    match args.get("params") {
        Some(p @ Value::Object(_)) => p.clone(),
        _ => json!({}),
    }
}

fn bad(m: &str) -> CmdError {
    CmdError { code: "bad_params", message: m.to_owned(), candidates: vec![] }
}

/// `structuredContent` must be an object.
fn wrap(v: Value) -> Value {
    if v.is_object() { v } else { json!({"result": v}) }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The `commands` tool: matches by words, or one command in full.
fn commands(args: &Value) -> Value {
    let kind = |k: Kind| match k {
        Kind::Query => "query",
        Kind::Library => "library",
        Kind::Live => "live",
    };
    if let Some(id) = args["id"].as_str() {
        return match spec(id) {
            Some(s) => json!({
                "id": s.id, "label": s.label, "kind": kind(s.kind), "destructive": s.destructive, "undo": s.undo,
                "main": s.main, "params": s.params, "inputSchema": crate::schema::input_schema(s),
            }),
            None => json!({"error": format!("no command {id:?}"), "like": commands(&json!({"filter": id}))["commands"]}),
        };
    }
    let words: Vec<String> = args["filter"].as_str().unwrap_or("").to_lowercase().split_whitespace().map(str::to_owned).collect();
    let found: Vec<Value> = COMMANDS
        .iter()
        .filter(|s| {
            let hay = format!("{} {}", s.id, s.label).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|s| json!({"id": s.id, "label": s.label, "kind": kind(s.kind)}))
        .collect();
    json!({"commands": found, "more": "commands {id} gives one command's parameters"})
}

/// Standard base64 (RFC 4648), for image content.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what reaches the app.
    #[derive(Default)]
    struct Fake {
        calls: Vec<(String, Value)>,
    }

    impl Backend for Fake {
        fn call(&mut self, command: &str, params: Value) -> Result<Value, CmdError> {
            self.calls.push((command.to_owned(), params));
            Ok(json!({"ran": command}))
        }
        fn screenshot(&mut self, _: Value) -> Result<Vec<u8>, CmdError> {
            Ok(b"PNG".to_vec())
        }
    }

    fn call(s: &mut Server<Fake>, name: &str, args: Value) -> Value {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}});
        s.message(&msg.to_string()).unwrap()["result"].clone()
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn the_handshake_and_tool_list() {
        let mut s = Server::new(Fake::default(), false);
        let init = s.message(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["capabilities"], json!({"tools": {}}));
        let newer = s.message(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#).unwrap();
        assert_eq!(newer["result"]["protocolVersion"], VERSIONS[0]);
        assert_eq!(s.message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
        let tools = s.message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, TOOLS);
        assert_eq!(s.message(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#).unwrap()["error"]["code"], -32601);
        assert_eq!(s.message(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#).unwrap()["error"]["code"], -32602);
        assert_eq!(s.message("{").unwrap()["error"]["code"], -32700);
    }

    #[test]
    fn tools_reach_the_backend_and_refuse_what_they_should() {
        let mut s = Server::new(Fake::default(), false);
        assert_eq!(call(&mut s, "query", json!({"command": "status"}))["structuredContent"]["ran"], "status");
        let r = call(&mut s, "query", json!({"command": "tile.trigger", "params": {"tile": "x"}}));
        assert_eq!(r["isError"], true, "only queries: {r}");
        let r = call(&mut s, "run", json!({"command": "tile.trigger", "params": {"tile": "Rimshot"}}));
        assert_eq!(r["structuredContent"]["ran"], "tile.trigger");
        let r = call(&mut s, "run", json!({"steps": [{"command": "set.create"}], "dryRun": true}));
        assert_eq!(r["structuredContent"]["ran"], "batch.run");
        assert_eq!(s.backend.calls.last().unwrap().1, json!({"steps": [{"command": "set.create"}], "dryRun": true}));
        for args in [json!({"command": "tile.remove", "params": {"tile": "x"}}), json!({"command": "app.quit"}), json!({"steps": [{"command": "set.create"}, {"command": "set.delete"}]})] {
            let r = call(&mut s, "run", args.clone());
            assert_eq!(r["isError"], true, "{args}: {r}");
            assert!(r["content"][0]["text"].as_str().unwrap().contains("refused"));
        }
        assert_eq!(s.backend.calls.len(), 3, "nothing refused reached the app");
        let mut open = Server::new(Fake::default(), true);
        assert_eq!(call(&mut open, "run", json!({"command": "tile.remove", "params": {"tile": "x"}}))["structuredContent"]["ran"], "tile.remove");
        let shot = call(&mut s, "screenshot", json!({}));
        assert_eq!(shot["content"][0], json!({"type": "image", "data": "UE5H", "mimeType": "image/png"}));
    }

    #[test]
    fn commands_finds_and_describes() {
        let found = commands(&json!({"filter": "tile fade"}));
        assert!(found["commands"].as_array().unwrap().iter().all(|c| c["id"].as_str().unwrap().contains("tile") || c["label"].as_str().unwrap().to_lowercase().contains("tile")));
        let one = commands(&json!({"id": "tile.trigger"}));
        assert_eq!(one["inputSchema"]["required"], json!(["tile"]));
        assert_eq!(one["kind"], "live");
        assert!(commands(&json!({"id": "tile.trigerr"}))["error"].is_string());
        // The whole list stays small enough to ask for (ids and labels only).
        assert!(commands(&json!({})).to_string().len() < 20_000);
    }
}
