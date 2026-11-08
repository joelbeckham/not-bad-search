//! Deterministic, read-only MCP client. No model calls or arbitrary tool execution.
use anyhow::{bail, ensure, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, io::Read, time::Duration};

pub const READ_TOOLS: [&str; 5] = [
    "jira_atlassianUserInfo",
    "jira_getAccessibleAtlassianResources",
    "jira_getVisibleJiraProjects",
    "jira_searchJiraIssuesUsingJql",
    "jira_getJiraIssue",
];
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
pub struct Connection {
    pub endpoint: String,
    pub api_key: String,
}
impl Connection {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.endpoint).context("Invalid MCP endpoint")?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
            "MCP endpoint must use HTTPS (HTTP is allowed for loopback testing)"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
            "Use the API key field for authentication"
        );
        ensure!(!self.api_key.trim().is_empty(), "Missing Bifrost API key");
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        crate::credentials::save_named("Bifrost/default", &serde_json::to_string(self)?)
    }
    pub fn load() -> Result<Self> {
        serde_json::from_str(
            &crate::credentials::load_named("Bifrost/default")?
                .context("Configure the Bifrost connection in Processing settings")?,
        )
        .context("Invalid saved Bifrost connection")
    }
}
pub struct Mcp {
    connection: Connection,
    client: Client,
    session: Option<String>,
    protocol: String,
    next_id: u64,
    tools: HashMap<String, (String, Value)>,
    discovered_tools: Vec<String>,
    discovered_schemas: HashMap<String, Value>,
}
pub fn allowed_name(name: &str) -> Option<&'static str> {
    if name.contains("service_user") {
        return None;
    }
    READ_TOOLS.into_iter().find(|canonical| {
        name == *canonical
            || name == canonical.replacen("jira_", "jira-", 1)
            || name
                .strip_suffix(canonical)
                .is_some_and(|prefix| prefix.ends_with("__"))
    })
}
/// MCP may encode a tool's JSON as structuredContent or JSON text blocks.
pub fn tool_payload(result: Value) -> Result<Value> {
    ensure!(
        result.get("isError").and_then(Value::as_bool) != Some(true),
        "MCP tool reported an error; verify Jira access and the source query"
    );
    if let Some(value) = result.get("structuredContent") {
        return Ok(value.clone());
    }
    let blocks = result
        .get("content")
        .and_then(Value::as_array)
        .context("MCP tool returned no structured content")?;
    let text = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&text).context("MCP tool did not return complete JSON")
}
impl Mcp {
    pub fn connect(connection: Connection) -> Result<Self> {
        Self::connect_inner(connection, true)
    }
    pub fn connect_discovery(connection: Connection) -> Result<Self> {
        Self::connect_inner(connection, false)
    }
    fn connect_inner(connection: Connection, require_sync: bool) -> Result<Self> {
        connection.validate()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut m = Self {
            connection,
            client,
            session: None,
            protocol: "2025-03-26".into(),
            next_id: 1,
            tools: HashMap::new(),
            discovered_tools: Vec::new(),
            discovered_schemas: HashMap::new(),
        };
        let result=m.rpc("initialize",json!({"protocolVersion":m.protocol,"capabilities":{},"clientInfo":{"name":"Good Search","version":"0.1.0"}}),false)?;
        m.protocol = result["protocolVersion"]
            .as_str()
            .context("MCP initialization omitted protocolVersion")?
            .into();
        m.rpc("notifications/initialized", json!({}), true)?;
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        let mut jira_names = Vec::new();
        for _ in 0..100 {
            let params = cursor.as_ref().map_or(json!({}), |c| json!({"cursor":c}));
            let list = m.rpc("tools/list", params, false)?;
            for tool in list["tools"].as_array().context("Invalid MCP tool list")? {
                let name = tool["name"].as_str().context("Unnamed MCP tool")?;
                m.discovered_tools.push(name.to_owned());
                if name.starts_with("jira_v2-") || name.to_lowercase().contains("slack") || name.to_lowercase().contains("gmail") {
                    m.discovered_schemas
                        .insert(name.to_owned(), tool["inputSchema"].clone());
                }
                if name.to_ascii_lowercase().contains("jira") {
                    jira_names.push(name.to_owned());
                }
                if let Some(canonical) = allowed_name(name) {
                    ensure!(!m.tools.contains_key(canonical),"Multiple ordinary Jira tools match {canonical}; use a connection exposing one Jira account");
                    m.tools
                        .insert(canonical.into(), (name.into(), tool["inputSchema"].clone()));
                }
            }
            cursor = list["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
            ensure!(
                seen.insert(cursor.clone()),
                "MCP tools/list repeated a continuation cursor"
            );
        }
        ensure!(cursor.is_none(), "MCP tool discovery exceeded 100 pages");
        for canonical in READ_TOOLS {
            let suffix = canonical.strip_prefix("jira_").unwrap();
            let name = format!("jira_v2-{suffix}");
            if let Some(schema) = m.discovered_schemas.get(&name) {
                m.tools.insert(canonical.into(), (name, schema.clone()));
            }
        }
        ensure!(
            !require_sync || (m.tools.contains_key("jira_searchJiraIssuesUsingJql")
                && m.tools.contains_key("jira_getJiraIssue")),
            "Ordinary Jira search/detail tools are not available through this connection. Discovered Jira tool names: {}",jira_names.join(", ")
        );
        Ok(m)
    }
    pub fn catalog(&self) -> Value {
        json!(self.tools)
    }
    pub fn gmail_read(&mut self, tool: &str, arguments: Value) -> Result<Value> {
        ensure!(["search_threads", "get_thread", "get_message", "list_labels"].contains(&tool), "Tool is outside the Gmail read allowlist");
        let name=format!("gmail-{tool}");
        ensure!(self.discovered_schemas.contains_key(&name), "Gmail tool unavailable: {name}");
        crate::bifrost::tool_payload(self.rpc("tools/call",serde_json::json!({"name":name,"arguments":arguments}),false)?)
    }
    pub fn slack_read(&mut self, tool: &str, arguments: Value) -> Result<Value> {
        ensure!(["slack_list_user_channels", "slack_read_channel", "slack_read_thread", "slack_read_file"].contains(&tool), "Tool is outside the Slack read allowlist");
        let name = format!("slack-{tool}");
        ensure!(self.discovered_schemas.contains_key(&name), "Required Slack tool unavailable: {tool}");
        let result = self.rpc("tools/call", json!({"name":name,"arguments":arguments}), false)?;
        ensure!(result["isError"].as_bool() != Some(true), "Slack read failed; verify channel access and connector configuration");
        Ok(result)
    }
    /// Discovery only: these names do not grant permission to call a tool.
    pub fn discovered_tools(&self) -> &[String] {
        &self.discovered_tools
    }
    pub fn v2_catalog(&self) -> Value {
        json!(self.discovered_schemas)
    }
    pub fn probe_v2_download(&mut self, arguments: Value) -> Result<Value> {
        let name = "jira_v2-downloadJiraIssueAttachment";
        ensure!(
            self.discovered_schemas.contains_key(name),
            "Jira v2 download tool unavailable"
        );
        let result = self.rpc(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            false,
        )?;
        if result["isError"].as_bool() == Some(true) {
            let message = result["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let safe = message
                .split_whitespace()
                .map(|word| {
                    if word.contains("http") {
                        "[URL omitted]"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            bail!(
                "Jira v2 download tool: {}",
                safe.chars().take(1500).collect::<String>()
            );
        }
        tool_payload(result)
    }
    pub fn supports(&self, tool: &str, field: &str) -> bool {
        self.tools
            .get(tool)
            .is_some_and(|(_, schema)| schema["properties"].get(field).is_some())
    }
    pub fn supports_adf(&self, tool: &str) -> bool {
        self.tools.get(tool).is_some_and(|(_,schema)| schema["properties"]["responseContentFormat"]["enum"].as_array().is_none_or(|values| values.iter().any(|v|v=="adf")))
    }
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value> {
        ensure!(
            READ_TOOLS.contains(&tool),
            "Tool is outside the Jira read allowlist"
        );
        let name = self
            .tools
            .get(tool)
            .context("Required Jira read tool is unavailable")?
            .0
            .clone();
        let result = tool_payload(self.rpc(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            false,
        )?)?;
        Ok(if name.starts_with("jira_v2-") || name.to_lowercase().contains("slack") {
            result.get("data").cloned().unwrap_or(result)
        } else {
            result
        })
    }
    fn rpc(&mut self, method: &str, params: Value, notification: bool) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let body = if notification {
            json!({"jsonrpc":"2.0","method":method,"params":params.clone()})
        } else {
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params.clone()})
        };
        let mut request = self
            .client
            .post(&self.connection.endpoint)
            .bearer_auth(&self.connection.api_key)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &self.protocol)
            .json(&body);
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        // Do not propagate reqwest errors containing the private endpoint URL.
        let response = request
            .send()
            .map_err(|_| anyhow::anyhow!("Bifrost network request failed or timed out"))?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "Bifrost HTTP {}{}",
                status.as_u16(),
                if matches!(status.as_u16(), 401 | 403) {
                    ": authentication or access denied"
                } else {
                    ""
                }
            );
        }
        if let Some(session) = response.headers().get("Mcp-Session-Id") {
            self.session = Some(session.to_str()?.into());
        }
        if notification {
            return Ok(Value::Null);
        }
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"));
        let mut bytes = Vec::new();
        response
            .take((MAX_RESPONSE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("Could not read Bifrost response"))?;
        ensure!(
            bytes.len() <= MAX_RESPONSE,
            "Bifrost response exceeded the 16 MiB limit; reduce page size or scope"
        );
        let value = decode_rpc(&bytes, sse, id)?;
        rpc_result(value)
    }
}
fn rpc_result(value: Value) -> Result<Value> {
    ensure!(value.get("error").is_none(),"Bifrost MCP request failed (JSON-RPC error); check credentials, tool availability and query");
    value
        .get("result")
        .cloned()
        .context("MCP response omitted result")
}
pub fn decode_rpc(bytes: &[u8], sse: bool, id: u64) -> Result<Value> {
    if !sse {
        let v: Value = serde_json::from_slice(bytes)?;
        ensure!(v["id"] == id, "Mismatched MCP response ID");
        return Ok(v);
    }
    let text = std::str::from_utf8(bytes)?.replace("\r\n", "\n");
    for event in text.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            if v["id"] == id {
                return Ok(v);
            }
        }
    }
    bail!("MCP event stream ended without a complete matching response")
}
