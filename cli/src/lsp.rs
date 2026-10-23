//! `suture lsp`: the language server (spec appendix P). It speaks the
//! Language Server Protocol over stdin and stdout: errors and warnings as
//! diagnostics, read-backs on hover, and the next words as completions.
//! A module is the `.st` files of one directory, as `suture build` reads it.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use suture_lang::editor;
use suture_lang::ir::Program;

#[derive(Default)]
struct Server {
    /// The open documents' text, by path.
    open: BTreeMap<PathBuf, String>,
    /// Each directory's last program that compiled, for hover and completion
    /// while the text has errors.
    programs: BTreeMap<PathBuf, Program>,
    shutdown: bool,
}

/// Reads one message: `Content-Length` headers, a blank line, a JSON body.
fn read(input: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, v)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let Some(n) = length else { return Ok(Some(Value::Null)) };
    // A module's files are small; a larger message is not read into memory.
    if n > MAX_MESSAGE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("a message of {n} bytes; at most {MAX_MESSAGE}")));
    }
    let mut body = vec![0; n];
    input.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// The largest message the server reads.
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

fn write(out: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let body = msg.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// A `file://` URI's path.
fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(Ok(b)) = bytes.get(i + 1..i + 3).map(|h| u8::from_str_radix(&String::from_utf8_lossy(h), 16))
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

fn uri_of(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_suture(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "st")
}

/// An LSP position's character (in UTF-16 units) as a character index.
fn column(line: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.chars().enumerate() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    line.chars().count()
}

impl Server {
    /// The `.st` files of `dir`, the open ones as the editor has them.
    fn files(&self, dir: &Path) -> Vec<(String, String)> {
        let mut out = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "st")
                    && let Ok(t) = std::fs::read_to_string(&p)
                {
                    out.insert(p, t);
                }
            }
        }
        for (p, t) in &self.open {
            if p.parent() == Some(dir) && is_suture(p) {
                out.insert(p.clone(), t.clone());
            }
        }
        out.into_iter().map(|(p, t)| (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), t)).collect()
    }

    /// Compiles the module `path` is in, and publishes its diagnostics for
    /// every file of it, so fixed ones are cleared.
    fn check(&mut self, path: &Path, out: &mut impl Write) -> std::io::Result<()> {
        let Some(dir) = path.parent() else { return Ok(()) };
        let files = self.files(dir);
        let (ds, prog) = editor::diagnostics(&files);
        if let Some(p) = prog {
            self.programs.insert(dir.to_path_buf(), p);
        }
        for (name, text) in &files {
            let lines: Vec<&str> = text.lines().collect();
            let items: Vec<Value> = ds
                .iter()
                .filter(|d| d.file == *name || (d.file.is_empty() && files.first().is_some_and(|f| f.0 == *name)))
                .map(|d| {
                    let l = d.line.saturating_sub(1);
                    let end = lines.get(l).map_or(0, |t| t.encode_utf16().count());
                    json!({
                        "range": { "start": { "line": l, "character": 0 }, "end": { "line": l, "character": end } },
                        "severity": if d.warning { 2 } else { 1 },
                        "source": "suture",
                        "message": d.msg,
                    })
                })
                .collect();
            let uri = uri_of(&dir.join(name));
            write(out, &json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": items } }))?;
        }
        Ok(())
    }

    /// The document, its program and the cursor (line from 1, column in
    /// characters) a request points at.
    fn at(&self, params: &Value) -> Option<(&String, Option<&Program>, usize, usize)> {
        let path = path_of(params["textDocument"]["uri"].as_str()?)?;
        let text = self.open.get(&path)?;
        let line = usize::try_from(params["position"]["line"].as_u64()?).ok()?;
        let col = column(text.lines().nth(line).unwrap_or(""), usize::try_from(params["position"]["character"].as_u64()?).ok()?);
        Some((text, path.parent().and_then(|d| self.programs.get(d)), line + 1, col))
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "capabilities": {
                    "textDocumentSync": 1,
                    "hoverProvider": true,
                    "completionProvider": { "triggerCharacters": [" ", "'"] },
                },
                "serverInfo": { "name": "suture", "version": env!("CARGO_PKG_VERSION") },
            })),
            "shutdown" => {
                self.shutdown = true;
                Ok(Value::Null)
            }
            "textDocument/hover" => Ok(match self.at(params) {
                Some((text, Some(p), line, col)) => {
                    editor::hover(p, text, line, col).map_or(Value::Null, |md| json!({ "contents": { "kind": "markdown", "value": md } }))
                }
                _ => Value::Null,
            }),
            "textDocument/completion" => {
                let Some((text, p, line, col)) = self.at(params) else { return Ok(json!([])) };
                let items: Vec<Value> = editor::complete(p, text, line, col)
                    .into_iter()
                    .map(|c| {
                        let (kind, format) = match c.detail.as_str() {
                            "statement" => (15, 2),
                            "kind" => (7, 1),
                            "adapter" => (9, 1),
                            "state" => (20, 1),
                            d if d.starts_with("attribute") => (10, 1),
                            _ => (14, 1),
                        };
                        json!({ "label": c.label, "detail": c.detail, "kind": kind, "insertText": c.insert, "insertTextFormat": format })
                    })
                    .collect();
                Ok(json!(items))
            }
            _ => Err((-32601, format!("{method} is not supported"))),
        }
    }

    fn notify(&mut self, method: &str, params: &Value, out: &mut impl Write) -> std::io::Result<()> {
        // Other files an editor opens beside a module are not part of it.
        let path = params["textDocument"]["uri"].as_str().and_then(path_of).filter(|p| is_suture(p));
        match (method, path) {
            ("textDocument/didOpen", Some(p)) => {
                self.open.insert(p.clone(), params["textDocument"]["text"].as_str().unwrap_or_default().to_string());
                self.check(&p, out)
            }
            ("textDocument/didChange", Some(p)) => {
                // Whole-text sync: the last change is the document.
                if let Some(t) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                    self.open.insert(p.clone(), t.to_string());
                }
                self.check(&p, out)
            }
            ("textDocument/didSave", Some(p)) => self.check(&p, out),
            ("textDocument/didClose", Some(p)) => {
                self.open.remove(&p);
                self.check(&p, out)
            }
            _ => Ok(()),
        }
    }
}

/// Serves one editor until it says `exit`.
pub fn run() -> Result<bool, String> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout().lock();
    let mut server = Server::default();
    let io = |e: std::io::Error| e.to_string();
    while let Some(msg) = read(&mut input).map_err(io)? {
        let method = msg["method"].as_str().unwrap_or_default().to_string();
        if method == "exit" {
            return Ok(server.shutdown);
        }
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match msg.get("id") {
            Some(id) if !method.is_empty() => {
                let reply = match server.request(&method, &params) {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
                };
                write(&mut out, &reply).map_err(io)?;
            }
            _ => server.notify(&method, &params, &mut out).map_err(io)?,
        }
    }
    Ok(server.shutdown)
}
