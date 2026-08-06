use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

pub const DEBOUNCE_WAIT: Duration = Duration::from_millis(350);

pub struct LspSession {
    child: Child,
    stdin: Option<ChildStdin>,
    receiver: mpsc::Receiver<Result<Value, String>>,
    next_id: i64,
    stopped: bool,
}

impl LspSession {
    pub fn spawn_lemma_lsp() -> Self {
        Self::spawn_lemma_lsp_args(&["lsp"])
    }

    /// Same as [`Self::spawn_lemma_lsp`], plus `--stdio` (vscode-languageclient flag).
    pub fn spawn_lemma_lsp_with_stdio_flag() -> Self {
        Self::spawn_lemma_lsp_args(&["lsp", "--stdio"])
    }

    fn spawn_lemma_lsp_args(args: &[&str]) -> Self {
        let bin = env!("CARGO_BIN_EXE_lemma");
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start lemma lsp");

        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut reader = stdout;
            loop {
                match read_lsp_message(&mut reader) {
                    Ok(text) => {
                        let value: Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = tx.send(Err(format!("invalid JSON: {e}: {text}")));
                                break;
                            }
                        };
                        if tx.send(Ok(value)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::UnexpectedEof {
                            let _ = tx.send(Err(format!("read error: {e}")));
                        }
                        break;
                    }
                }
            }
        });

        LspSession {
            stdin: Some(child.stdin.take().expect("stdin")),
            child,
            receiver: rx,
            next_id: 0,
            stopped: false,
        }
    }

    pub fn initialize(&mut self, root_uri: Option<&str>) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "lemma-lsp-test", "version": "1.0" },
            "capabilities": {},
            "rootUri": root_uri
        });
        self.send_request(id, "initialize", params)
    }

    pub fn initialized(&mut self) {
        self.send_notification("initialized", json!({}));
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send_request(id, method, params)
    }

    pub fn send_notification(&mut self, method: &str, params: Value) {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }));
    }

    pub fn did_open(&mut self, uri: &str, text: &str) {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "lemma",
                    "version": 1,
                    "text": text
                }
            }),
        );
    }

    pub fn did_close(&mut self, uri: &str) {
        self.send_notification(
            "textDocument/didClose",
            json!({
                "textDocument": { "uri": uri }
            }),
        );
    }

    pub fn wait_for_diagnostics(&mut self, uri: &str, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = match self.recv_message(remaining) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("LSP reader disconnected while waiting for publishDiagnostics");
                }
            };
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
                && msg["params"]["uri"].as_str() == Some(uri)
            {
                return msg;
            }
        }
        panic!("timed out waiting for publishDiagnostics for {uri}");
    }

    pub fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        if self.stdin.is_some() {
            self.next_id += 1;
            let id = self.next_id;
            self.write_message(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "shutdown"
            }));
            let _ = self.wait_response(id, Duration::from_secs(5));
            self.write_message(json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }));
        }
        drop(self.stdin.take());
        self.child.kill().ok();
        self.child.wait().ok();
        self.stopped = true;
    }

    fn send_request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }));
        self.wait_response(id, Duration::from_secs(5))
    }

    fn wait_response(&mut self, id: i64, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = match self.recv_message(remaining) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("LSP reader disconnected while waiting for response id {id}");
                }
            };
            if msg.get("id") == Some(&json!(id)) {
                assert!(
                    msg.get("error").is_none(),
                    "LSP error for request {id}: {}",
                    msg["error"]
                );
                return msg["result"].clone();
            }
        }
        panic!("timed out waiting for response id {id}");
    }

    fn recv_message(&mut self, timeout: Duration) -> Result<Value, RecvTimeoutError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => panic!("LSP reader error: {e}"),
            Err(e) => Err(e),
        }
    }

    fn write_message(&mut self, message: Value) {
        let stdin = self
            .stdin
            .as_mut()
            .expect("BUG: write_message after session stopped");
        let body = message.to_string();
        let payload = encode_lsp_message(&body);
        stdin.write_all(payload.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        if !self.stopped {
            drop(self.stdin.take());
            self.child.kill().ok();
            self.child.wait().ok();
        }
    }
}

pub fn path_to_uri(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = path.to_string_lossy().replace('\\', "/");
    if path_str.starts_with('/') {
        format!("file://{path_str}")
    } else {
        format!("file:///{path_str}")
    }
}

pub fn write_lemma_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("write lemma file");
    path
}

fn encode_lsp_message(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn read_lsp_message(reader: &mut impl Read) -> std::io::Result<String> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 8192 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "LSP header too large",
            ));
        }
    }

    let header_text = String::from_utf8_lossy(&header);
    let content_length = header_text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "LSP message missing Content-Length",
            )
        })?;

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
