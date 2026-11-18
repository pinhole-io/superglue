//! A reusable MCP tool server.
//!
//! The caller owns the tools. This module owns the protocol and the byte stream.
//! Stdio is one transport. Any split reader and writer is another.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    ServerInfo, ToolsCapability,
};
use rmcp::service::ServiceExt;
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{RoleServer, ServerHandler};
use serde_json::{Map, Value};
use tokio::io::{AsyncRead, AsyncWrite};

use super::McpError;

/// A synchronous tool body. The server runs it off the async worker.
pub type ToolFn = Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;

/// One tool the server advertises.
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Map<String, Value>,
    pub call: ToolFn,
}

struct Registered {
    name: String,
    description: String,
    schema: Map<String, Value>,
    call: ToolFn,
}

impl Clone for Registered {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            schema: self.schema.clone(),
            call: Arc::clone(&self.call),
        }
    }
}

struct Inner {
    name: String,
    tools: Vec<Registered>,
}

/// MCP server that lists and calls the registered tools.
#[derive(Clone)]
pub struct ToolServer {
    inner: Arc<Inner>,
}

impl ToolServer {
    /// Build a server with the name clients see at initialize.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                name: name.into(),
                tools: Vec::new(),
            }),
        }
    }

    /// Add one tool. A repeated name replaces the earlier tool.
    pub fn tool(self, def: ToolDef) -> Self {
        let mut tools = self.inner.tools.clone();
        if let Some(slot) = tools.iter_mut().find(|tool| tool.name == def.name) {
            *slot = registered(def);
        } else {
            tools.push(registered(def));
        }
        Self {
            inner: Arc::new(Inner {
                name: self.inner.name.clone(),
                tools,
            }),
        }
    }

    /// Names in advertisement order.
    pub fn names(&self) -> Vec<String> {
        self.inner
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect()
    }

    /// Call one tool. An unknown name is an error the caller can show.
    pub fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let Some(tool) = self.inner.tools.iter().find(|tool| tool.name == name) else {
            return Err(format!("unknown tool {name}"));
        };
        (tool.call)(args)
    }

    /// Serve one MCP session on the process stdio streams.
    pub async fn serve_stdio(self) -> Result<(), McpError> {
        let (read, write) = rmcp::transport::stdio();
        self.serve_rw(read, write).await
    }

    /// Serve one MCP session on a split byte stream.
    pub async fn serve_rw<R, W>(self, read: R, write: W) -> Result<(), McpError>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let transport = AsyncRwTransport::<RoleServer, _, _>::new_server(read, write);
        let running = self
            .serve(transport)
            .await
            .map_err(|error| McpError::Connect(error.to_string()))?;
        running
            .waiting()
            .await
            .map_err(|error| McpError::Connect(error.to_string()))?;
        Ok(())
    }
}

fn registered(def: ToolDef) -> Registered {
    Registered {
        name: def.name,
        description: def.description,
        schema: def.input_schema,
        call: def.call,
    }
}

impl ServerHandler for ToolServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new(self.inner.name.clone(), env!("CARGO_PKG_VERSION"));
        info.capabilities.tools = Some(ToolsCapability::default());
        info
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        let tools = self
            .inner
            .tools
            .iter()
            .map(|tool| {
                rmcp::model::Tool::new(
                    tool.name.clone(),
                    tool.description.clone(),
                    Arc::new(tool.schema.clone()),
                )
            })
            .collect();
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        let name = request.name.to_string();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let server = self.clone();
        async move {
            let result = tokio::task::spawn_blocking(move || server.call(&name, args))
                .await
                .map_err(|error| rmcp::ErrorData::internal_error(error.to_string(), None))?;
            Ok(match result {
                Ok(value) => CallToolResult::success(vec![ContentBlock::text(json_text(&value))]),
                Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
            })
        }
    }
}

fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use rmcp::{RoleClient, ServiceExt};

    fn echo_server() -> ToolServer {
        ToolServer::new("probe").tool(ToolDef {
            name: "echo".to_string(),
            description: "Return the arguments.".to_string(),
            input_schema: Map::new(),
            call: Arc::new(|args| Ok(args)),
        })
    }

    #[test]
    fn a_call_reaches_the_registered_tool() {
        let server = echo_server();
        assert_eq!(server.names(), vec!["echo".to_string()]);
        let value = server
            .call("echo", serde_json::json!({"n": 1}))
            .expect("echo");
        assert_eq!(value["n"], 1);
        let missing = server.call("missing", Value::Null).expect_err("missing");
        assert!(missing.contains("unknown tool"));
    }

    #[tokio::test]
    async fn a_client_lists_and_calls_a_tool() {
        let server = echo_server();
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_io);
        let (client_read, client_write) = tokio::io::split(client_io);
        let served = tokio::spawn(async move { server.serve_rw(server_read, server_write).await });
        let client = ()
            .serve(AsyncRwTransport::<RoleClient, _, _>::new_client(
                client_read,
                client_write,
            ))
            .await
            .expect("client");
        let tools = client.peer().list_all_tools().await.expect("list");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let result = client
            .peer()
            .call_tool(CallToolRequestParams::new("echo").with_arguments({
                let mut args = Map::new();
                args.insert("n".to_string(), Value::from(2));
                args
            }))
            .await
            .expect("call");
        assert_ne!(result.is_error, Some(true));
        let text = serde_json::to_string(&result).expect("result json");
        assert!(text.contains('2'), "{text}");
        client.cancel().await.expect("cancel");
        served.await.expect("join").expect("server");
    }
}
