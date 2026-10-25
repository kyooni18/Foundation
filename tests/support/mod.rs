use prime_radiant::{authentication::Credentials, domain::PrincipalKind};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};
pub struct Server {
    pub _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub endpoint: String,
    pub tokens: Vec<String>,
    pub child: Child,
    runtime: tokio::runtime::Runtime,
    http: reqwest::Client,
}
impl Server {
    pub fn start() -> Self {
        Self::start_with_setup(|_| {})
    }
    pub fn start_with_setup(setup: impl FnOnce(&std::path::Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (i, kind) in [
            PrincipalKind::User,
            PrincipalKind::Agent,
            PrincipalKind::Import,
            PrincipalKind::Agent,
        ]
        .iter()
        .enumerate()
        {
            Credentials::issue(
                root.join("credentials"),
                root.join(format!("token{i}")),
                *kind,
                Some(format!("trusted{i}")),
            )
            .unwrap();
        }
        let tokens = (0..4)
            .map(|i| fs::read_to_string(root.join(format!("token{i}"))).unwrap())
            .collect();
        setup(&root);
        let mut child = Command::new(env!("CARGO_BIN_EXE_primeradiant"))
            .args(["--db"])
            .arg(root.join("db"))
            .args(["serve", "--credentials"])
            .arg(root.join("credentials"))
            .args(["--bind", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let line = line.unwrap();
                if let Some(addr) = line.strip_prefix("listening ") {
                    let _ = tx.send(addr.to_string());
                }
            }
        });
        let address = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("HTTP server startup");
        Self {
            _dir: dir,
            root,
            endpoint: format!("http://{address}"),
            tokens,
            child,
            runtime: tokio::runtime::Runtime::new().unwrap(),
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }
    pub fn request(&self, token: Option<&str>, op: &str, args: Value) -> (u16, Value) {
        self.runtime.block_on(async {
            let mut request = self
                .http
                .post(format!("{}/v1/{op}", self.endpoint))
                .json(&args);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            assert_eq!(response.headers()["cache-control"], "no-store");
            (status, response.json().await.unwrap())
        })
    }
    pub fn call(&self, role: usize, op: &str, args: Value) -> Value {
        self.request(Some(&self.tokens[role]), op, args).1
    }
    pub fn record(&self, role: usize, text: &str) -> String {
        self.call(role, "record", json!({"content":text}))["result"]["source_id"]
            .as_str()
            .unwrap()
            .into()
    }
    pub fn mcp(&self, role: usize) -> Mcp {
        Mcp::start(&self.endpoint, self.root.join(format!("token{role}")), true)
    }
    pub fn mcp_compact(&self, role: usize) -> Mcp {
        Mcp::start(
            &self.endpoint,
            self.root.join(format!("token{role}")),
            false,
        )
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub struct Mcp {
    child: Child,
    input: ChildStdin,
    output: Receiver<String>,
    id: u64,
}
impl Mcp {
    fn start(endpoint: &str, token: PathBuf, full_tools: bool) -> Self {
        let mut client = Self::uninitialized_with_tools(endpoint, token, full_tools);
        let init=client.rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        client.notify("notifications/initialized", json!({}));
        client
    }
    pub fn uninitialized(endpoint: &str, token: PathBuf) -> Self {
        Self::uninitialized_with_tools(endpoint, token, true)
    }
    fn uninitialized_with_tools(endpoint: &str, token: PathBuf, full_tools: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_primeradiant"));
        command
            .args(["mcp", "--server", endpoint, "--token-file"])
            .arg(token);
        if full_tools {
            command.arg("--full-tools");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            id: 0,
        }
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{}", value).unwrap();
        self.input.flush().unwrap();
    }
    pub fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        let line = self
            .output
            .recv_timeout(Duration::from_secs(15))
            .expect("MCP response");
        let response: Value = serde_json::from_str(&line).expect("stdout must contain only JSON");
        assert_eq!(response["id"], self.id);
        response
    }
    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}));
    }
    pub fn call(&mut self, op: &str, args: Value) -> Value {
        let response = self.rpc("tools/call", json!({"name":op,"arguments":args}));
        let result = &response["result"];
        let value: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().expect("tool result"))
                .unwrap();
        assert_eq!(&value, &result["structuredContent"]);
        assert_eq!(result["isError"], !value["ok"].as_bool().unwrap());
        value
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
