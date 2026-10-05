//! Async tool registry and dispatch.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::instrument;

use super::error::ToolInvokeError;
use super::types::{ToolContextPolicy, ToolRetryPolicy, ToolSpec};

/// Async tool implementation (JSON args in, JSON value out).
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError>;

    /// Return whether the tool is currently available for discovery and invocation.
    fn is_available(&self) -> bool {
        true
    }

    /// Whether PostTool offload may replace this tool's result with a notepad stub.
    fn context_policy(&self) -> ToolContextPolicy {
        ToolContextPolicy::OffloadWhenLarge
    }
}

/// Registry of tools keyed by name, each with an optional per-tool error policy.
#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, (Arc<dyn Tool>, ToolRetryPolicy)>>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Register a tool with the default (fail-fast) error policy.
    ///
    /// Returns an error if the tool name is already registered.
    pub async fn register(&self, tool: Arc<dyn Tool>) -> Result<(), ToolInvokeError> {
        self.register_with_policy(tool, ToolRetryPolicy::default())
            .await
    }

    /// Register a tool with an explicit per-tool error policy.
    ///
    /// Returns an error if the tool name is already registered.
    pub async fn register_with_policy(
        &self,
        tool: Arc<dyn Tool>,
        policy: ToolRetryPolicy,
    ) -> Result<(), ToolInvokeError> {
        let name = tool.spec().name.clone();
        let mut map = self.tools.write().await;
        if map.contains_key(&name) {
            return Err(ToolInvokeError::DuplicateRegistration { name });
        }
        map.insert(name, (tool, policy));
        Ok(())
    }

    /// List registered tool specs sorted by name so the provider `tools[]`
    /// prefix stays byte-stable across restarts and sub-agents.
    pub async fn list_specs(&self) -> Vec<ToolSpec> {
        let map = self.tools.read().await;
        let mut specs: Vec<ToolSpec> = map
            .values()
            .filter(|(tool, _)| tool.is_available())
            .map(|(tool, _)| tool.spec())
            .collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    /// Return the error policy registered for `name`, or `ToolRetryPolicy::default()`.
    pub async fn policy_for(&self, name: &str) -> ToolRetryPolicy {
        let map = self.tools.read().await;
        map.get(name).map(|(_, p)| p.clone()).unwrap_or_default()
    }

    /// Unregister tools by name. Returns the number of tools actually removed.
    pub async fn unregister(&self, names: &[String]) -> usize {
        let mut map = self.tools.write().await;
        let mut removed = 0;
        for name in names {
            if map.remove(name).is_some() {
                removed += 1;
            }
        }
        removed
    }

    /// Resolve a tool and its error policy in one registry read.
    pub async fn resolve_invocation(
        &self,
        name: &str,
    ) -> Result<(Arc<dyn Tool>, ToolRetryPolicy), ToolInvokeError> {
        let map = self.tools.read().await;
        match map.get(name) {
            Some((tool, policy)) if tool.is_available() => Ok((Arc::clone(tool), policy.clone())),
            None => Err(ToolInvokeError::UnknownTool {
                name: name.to_string(),
            }),
            Some(_) => Err(ToolInvokeError::UnknownTool {
                name: name.to_string(),
            }),
        }
    }

    /// Invoke a tool by name.
    #[instrument(skip(self, arguments), fields(tool = name))]
    pub async fn invoke(&self, name: &str, arguments: Value) -> Result<Value, ToolInvokeError> {
        crate::telemetry::openinference::tag_tool("", name, "");
        let tool = match self.resolve_invocation(name).await {
            Ok((tool, _)) => tool,
            Err(error) => {
                crate::telemetry::openinference::fail_current("unknown tool");
                return Err(error);
            }
        };
        if !tool.is_available() {
            crate::telemetry::openinference::fail_current("unknown tool");
            return Err(ToolInvokeError::UnknownTool {
                name: name.to_string(),
            });
        }
        let result = tool.call(arguments).await;
        if let Err(err) = &result {
            crate::telemetry::openinference::fail_current(tool_error_class(err));
        }
        result
    }
}

fn tool_error_class(error: &ToolInvokeError) -> &'static str {
    match error {
        ToolInvokeError::HandlerFailed { .. } => "tool handler failed",
        ToolInvokeError::UnknownTool { .. } => "unknown tool",
        ToolInvokeError::DuplicateRegistration { .. } => "duplicate tool registration",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct EchoTool {
        name: String,
    }

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.clone(),
                description: Some("echo".into()),
                parameters_schema: serde_json::json!({"type": "object", "properties": {}}),
                static_tool: false,
            }
        }

        async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
            Ok(arguments)
        }
    }

    fn make_tool(name: &str) -> Arc<dyn Tool> {
        Arc::new(EchoTool { name: name.into() })
    }

    struct GatedTool {
        available: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl Tool for GatedTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("gated", json!({"type": "object"}))
        }

        async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
            Ok(arguments)
        }

        fn is_available(&self) -> bool {
            self.available.load(Ordering::Acquire)
        }
    }

    #[tokio::test]
    async fn unregister_removes_tools() {
        let registry = ToolRegistry::new();
        registry.register(make_tool("foo")).await.unwrap();
        registry.register(make_tool("bar")).await.unwrap();

        let removed = registry.unregister(&["foo".into(), "bar".into()]).await;
        assert_eq!(removed, 2);

        let specs = registry.list_specs().await;
        assert!(specs.is_empty());
    }

    #[tokio::test]
    async fn unregister_invoke_returns_unknown_tool() {
        let registry = ToolRegistry::new();
        registry.register(make_tool("foo")).await.unwrap();
        registry.unregister(&["foo".into()]).await;

        let result = registry.invoke("foo", json!({})).await;
        assert!(matches!(result, Err(ToolInvokeError::UnknownTool { name }) if name == "foo"));
    }

    #[tokio::test]
    async fn unregister_unknown_name_returns_zero() {
        let registry = ToolRegistry::new();
        let removed = registry.unregister(&["nonexistent".into()]).await;
        assert_eq!(removed, 0);
    }

    #[tokio::test]
    async fn unregister_partial_names() {
        let registry = ToolRegistry::new();
        registry.register(make_tool("a")).await.unwrap();
        registry.register(make_tool("b")).await.unwrap();

        let removed = registry.unregister(&["a".into(), "missing".into()]).await;
        assert_eq!(removed, 1);

        let specs = registry.list_specs().await;
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "b");
    }

    #[tokio::test]
    async fn list_specs_is_sorted_by_name() {
        let registry = ToolRegistry::new();
        registry.register(make_tool("zeta")).await.unwrap();
        registry.register(make_tool("alpha")).await.unwrap();
        registry.register(make_tool("mu")).await.unwrap();
        let names: Vec<String> = registry
            .list_specs()
            .await
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    #[tokio::test]
    async fn unavailable_tools_are_hidden_and_rejected() {
        let available = Arc::new(AtomicBool::new(false));
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(GatedTool {
                available: Arc::clone(&available),
            }))
            .await
            .unwrap();

        assert!(registry.list_specs().await.is_empty());
        assert!(matches!(
            registry.resolve_invocation("gated").await,
            Err(ToolInvokeError::UnknownTool { name }) if name == "gated"
        ));
        assert!(matches!(
            registry.invoke("gated", json!({})).await,
            Err(ToolInvokeError::UnknownTool { name }) if name == "gated"
        ));

        available.store(true, Ordering::Release);
        assert_eq!(registry.list_specs().await[0].name, "gated");
        assert_eq!(
            registry.invoke("gated", json!({"ok": true})).await.unwrap(),
            json!({"ok": true})
        );
    }

    #[test]
    fn telemetry_error_class_never_contains_handler_payload() {
        let error =
            ToolInvokeError::handler("marker-password-7e4f", Some("secret-code".to_string()));
        assert_eq!(tool_error_class(&error), "tool handler failed");
    }
}
