//! rust-analyzer for the editor's Rust completion (operator, 2026-11-01: "the
//! expected interactivity expected of a modern ide - file watcher for its
//! file type, complete text within context of file, arg completion, etc for
//! rust, R, julia, python, or shell"). R, Python, Julia and the shell
//! complete in their sessions (kernel.rs `complete`); Rust has no session, so
//! a language server answers: one rust-analyzer per crate (the folder of the
//! nearest Cargo.toml), started by the first completion asked in it, told the
//! file's text before each ask, and stopped after IDLE without one. At most
//! MAX of them run: each holds the crate's index in memory.
//!
//! Build scripts and proc-macros are off: they compile and run code, and on
//! a shared machine that memory goes to people's work first. Completion of
//! what a derive adds is the cost.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::api::Completion;

const IDLE: Duration = Duration::from_secs(10 * 60);
const MAX: usize = 2;
/// How long one completion waits: past it, the crate is still being read.
const WAIT: Duration = Duration::from_secs(4);

type Waiting = Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>;

struct Server {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next: i64,
    waiting: Waiting,
    /// Each open file's version, as rust-analyzer was last told it.
    versions: HashMap<String, i64>,
    used: Instant,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn servers() -> &'static Mutex<HashMap<PathBuf, Server>> {
    static S: OnceLock<Mutex<HashMap<PathBuf, Server>>> = OnceLock::new();
    S.get_or_init(|| {
        // Idle ones go, whether or not anything is asked again.
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(60));
            servers().lock().unwrap().retain(|_, s| s.used.elapsed() < IDLE);
        });
        Mutex::default()
    })
}

/// The crate a file is in: the nearest folder above it with a Cargo.toml.
fn crate_root(file: &Path) -> Option<PathBuf> {
    file.ancestors().skip(1).find(|d| d.join("Cargo.toml").is_file()).map(Path::to_path_buf)
}

/// A file:// URI, its path percent-encoded where it must be.
fn uri(p: &Path) -> String {
    let s = p.display().to_string();
    let enc: String = s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("file://{enc}")
}

fn write(stdin: &Mutex<ChildStdin>, v: &Value) -> Result<(), String> {
    let body = v.to_string();
    let mut w = stdin.lock().unwrap();
    w.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).and_then(|_| w.flush()).map_err(|e| format!("rust-analyzer: {e}"))
}

/// Reads rust-analyzer's messages: an answer goes to whoever waits on its
/// id; a request of the server's own is answered "nothing" so it never
/// waits on us; notifications (progress, diagnostics) are let go.
fn read(out: impl Read + Send + 'static, waiting: Waiting, stdin: Arc<Mutex<ChildStdin>>) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(out);
        loop {
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                match r.read_line(&mut h) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let h = h.trim();
                if h.is_empty() {
                    break;
                }
                if let Some(n) = h.strip_prefix("Content-Length:") {
                    len = n.trim().parse().unwrap_or(0);
                }
            }
            let mut buf = vec![0; len];
            if r.read_exact(&mut buf).is_err() {
                return;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&buf) else { continue };
            let Some(id) = v.get("id").filter(|i| !i.is_null()) else { continue };
            if v.get("method").is_some() {
                let _ = write(&stdin, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
            } else if let Some(tx) = id.as_i64().and_then(|i| waiting.lock().unwrap().remove(&i)) {
                let _ = tx.send(v);
            }
        }
    });
}

impl Server {
    fn start(root: &Path) -> Result<Server, String> {
        // The stack's Rust brings rust-analyzer beside cargo; else PATH's.
        let enter = crate::host::use_sh().map(|p| format!(". {} >/dev/null 2>&1; ml Rust >/dev/null 2>&1; ", crate::api::shell_quote(&p.display().to_string()))).unwrap_or_default();
        let mut child = Command::new("bash")
            .args(["-c", &format!("set +u; {enter}exec rust-analyzer")])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("rust-analyzer: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("rust-analyzer: no stdin")?));
        let waiting: Waiting = Default::default();
        read(child.stdout.take().ok_or("rust-analyzer: no stdout")?, waiting.clone(), stdin.clone());
        let mut s = Server { child, stdin, next: 1, waiting, versions: HashMap::new(), used: Instant::now() };
        let init = json!({
            "processId": std::process::id(),
            "rootUri": uri(root),
            // This rust-analyzer loads a project only from the workspace
            // folders: rootUri alone answered every completion with null.
            "workspaceFolders": [{"uri": uri(root), "name": root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()}],
            "capabilities": {"textDocument": {"completion": {"completionItem": {"snippetSupport": false}}}},
            "initializationOptions": {
                "checkOnSave": false,
                "cargo": {"buildScripts": {"enable": false}},
                "procMacro": {"enable": false},
                "cachePriming": {"enable": false}
            }
        });
        s.request("initialize", init, Duration::from_secs(30)).map_err(|e| if e.contains("still") { "rust-analyzer did not start (is the stack's Rust module there?)".to_owned() } else { e })?;
        s.notify("initialized", json!({}))?;
        Ok(s)
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write(&self.stdin, &json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn request(&mut self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id, tx);
        write(&self.stdin, &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        match rx.recv_timeout(wait) {
            Ok(v) => match v.get("error") {
                Some(e) => Err(format!("rust-analyzer: {}", e["message"].as_str().unwrap_or("refused"))),
                None => Ok(v["result"].clone()),
            },
            Err(_) => {
                self.waiting.lock().unwrap().remove(&id);
                Err("rust-analyzer is still reading the crate - ask again in a moment".into())
            }
        }
    }

    /// What the editor has, told as the file's newest version.
    fn tell(&mut self, uri: &str, text: &str) -> Result<(), String> {
        match self.versions.get_mut(uri) {
            None => {
                self.versions.insert(uri.to_owned(), 1);
                self.notify("textDocument/didOpen", json!({"textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": text}}))
            }
            Some(v) => {
                *v += 1;
                let v = *v;
                self.notify("textDocument/didChange", json!({"textDocument": {"uri": uri, "version": v}, "contentChanges": [{"text": text}]}))
            }
        }
    }
}

/// Completions at line `row`, character `col` of Rust file `path`, whose text
/// in the editor is `text` (saved or not).
pub fn complete_rust(path: &str, text: &str, row: usize, col: usize) -> Result<Completion, String> {
    let file = PathBuf::from(path);
    let root = crate_root(&file).ok_or("not in a Cargo crate: no Cargo.toml above this file")?;
    let mut all = servers().lock().unwrap();
    all.retain(|_, s| s.used.elapsed() < IDLE && s.alive());
    if !all.contains_key(&root) {
        while all.len() >= MAX {
            let Some(oldest) = all.iter().min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) else { break };
            all.remove(&oldest);
        }
        all.insert(root.clone(), Server::start(&root)?);
    }
    let s = all.get_mut(&root).expect("just put there");
    s.used = Instant::now();
    let u = uri(&file);
    s.tell(&u, text)?;
    let line = text.split('\n').nth(row).unwrap_or("");
    // LSP counts a line's characters in UTF-16 units.
    let character: usize = line.chars().take(col).map(char::len_utf16).sum();
    let got = s.request("textDocument/completion", json!({"textDocument": {"uri": u}, "position": {"line": row, "character": character}}), WAIT)?;
    Ok(items(&got, line, col))
}

/// The answer as the editor wants it: the word being typed (letters, digits
/// and _ before the cursor) is what an item replaces; items that do not
/// start with it go, and the rest keep rust-analyzer's order.
fn items(got: &Value, line: &str, col: usize) -> Completion {
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len());
    let mut start = col;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let typed: String = chars[start..col].iter().collect::<String>().to_lowercase();
    let list = got.get("items").unwrap_or(got).as_array().cloned().unwrap_or_default();
    let mut v: Vec<(String, String, String)> = list
        .iter()
        .filter_map(|it| {
            let text = it["textEdit"]["newText"].as_str().or(it["insertText"].as_str()).or(it["label"].as_str())?.to_owned();
            let key = it["filterText"].as_str().unwrap_or(&text).to_lowercase();
            if !key.starts_with(&typed) {
                return None;
            }
            let detail = it["detail"].as_str().or(it["labelDetails"]["detail"].as_str()).unwrap_or("").to_owned();
            let sort = it["sortText"].as_str().unwrap_or(&text).to_owned();
            Some((sort, text, detail))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.dedup_by(|a, b| a.1 == b.1);
    v.truncate(100);
    Completion { start, items: v.iter().map(|x| x.1.clone()).collect(), detail: v.into_iter().map(|x| x.2).collect() }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn an_answer_is_cut_to_the_word_typed() {
        let got = json!({"isIncomplete": false, "items": [
            {"label": "push", "insertText": "push", "detail": "fn(&mut self, T)", "sortText": "1"},
            {"label": "pop", "insertText": "pop", "detail": "fn(&mut self) -> Option<T>", "sortText": "0"},
            {"label": "push_str(…)", "textEdit": {"newText": "push_str", "range": {}}, "filterText": "push_str", "sortText": "2"}
        ]});
        let c = super::items(&got, "    v.pu", 8);
        assert_eq!(c.start, 6);
        assert_eq!(c.items, vec!["push", "push_str"]);
        assert_eq!(c.detail[0], "fn(&mut self, T)");
    }

    #[test]
    fn a_file_outside_a_crate_says_so() {
        let e = super::complete_rust("/nonexistent/x/main.rs", "fn main() {}", 0, 3).unwrap_err();
        assert!(e.contains("Cargo"), "{e}");
    }

    /// A real rust-analyzer from the stack on a two-line crate (`cargo test
    /// rust_analyzer_live -- --ignored`): a local name completes within the
    /// first few asks, as the crate is read.
    #[test]
    #[ignore]
    fn rust_analyzer_live() {
        let d = std::env::temp_dir().join(format!("cove-ra-{}", std::process::id()));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"ra\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        let text = "fn main() {\n    let penguins = 3;\n    peng\n}\n";
        std::fs::write(d.join("src/main.rs"), text).unwrap();
        let f = d.join("src/main.rs").display().to_string();
        let mut got = Vec::new();
        for _ in 0..10 {
            if let Ok(c) = super::complete_rust(&f, text, 2, 8) {
                got = c.items;
                if !got.is_empty() {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        let _ = std::fs::remove_dir_all(&d);
        assert!(got.iter().any(|i| i == "penguins"), "{got:?}");
    }

    #[test]
    fn a_path_is_a_file_uri() {
        assert_eq!(super::uri(std::path::Path::new("/home/a b/src/main.rs")), "file:///home/a%20b/src/main.rs");
    }
}
