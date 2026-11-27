//! Anthropic Messages API adapter.

use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::chat::SystemPromptBlock;
use crate::http::join_base_url;
use crate::openai::{
    ChatMessage, ContentPart, FileContent, FunctionCall, MessageContent, ToolCall,
};
use crate::tools::ToolSpec;

use super::adapter::{
    LlmProvider, NormalizedCompletion, NormalizedResponse, ProviderParseError, ProviderRequest,
    ProviderRequestContext, ProviderResponsesContext, rate_limit_key_for,
};
use super::model_ref::wire_model_id;
use super::provider_id::ProviderId;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const CACHE_CONTROL_EPHEMERAL: &str = "ephemeral";

#[derive(Debug, Clone, Copy)]
pub struct AnthropicProvider;

impl AnthropicProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl LlmProvider for AnthropicProvider {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    fn build_chat_request(&self, ctx: &ProviderRequestContext<'_>) -> ProviderRequest {
        let base_url = ctx.credentials.base_url_for(ProviderId::Anthropic);
        let url = join_base_url(&base_url, "/v1/messages");
        let api_key = ctx
            .credentials
            .key_for(ProviderId::Anthropic)
            .expect("credentials checked before build");
        let headers = vec![
            ("x-api-key".to_string(), api_key.expose_secret().to_string()),
            (
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];

        let (legacy_system, messages) = map_messages(ctx.messages);
        let max_tokens = ctx.options.max_completion_tokens.unwrap_or(4096);
        let cache_tools = should_cache_tools(ctx);
        let wire_model = wire_model_id(ctx.model_ref, &base_url, ProviderId::Anthropic);

        let mut body = json!({
            "model": wire_model,
            "max_tokens": max_tokens,
            "messages": messages,
        });

        if let Some(system) = build_system_payload(ctx, legacy_system.as_deref()) {
            body["system"] = system;
        }

        if let Some(specs) = ctx.tools {
            body["tools"] = json!(build_tools(specs, cache_tools));
        }

        if ctx.stream {
            body["stream"] = json!(true);
        }

        if anthropic_supports_sampling_params(&ctx.model_ref.model)
            && let Some(temp) = ctx.options.temperature
        {
            body["temperature"] = json!(temp);
        }

        apply_thinking_and_extra(&mut body, ctx.options, max_tokens);

        let rate_limit_key =
            rate_limit_key_for(ctx.model_ref, ctx.credentials).expect("credentials checked");

        ProviderRequest {
            url,
            headers,
            body,
            rate_limit_key,
            model_ref: ctx.model_ref.clone(),
            bare_model: ctx.model_ref.model.clone(),
        }
    }

    fn parse_chat_response(
        &self,
        json: &Value,
    ) -> Result<NormalizedCompletion, ProviderParseError> {
        let resp = AnthropicMessageResponse::deserialize(json)?;
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut provider_blocks = Vec::new();
        for block in &resp.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    text_parts.push(text.clone());
                    provider_blocks.push(json!({"type": "text", "text": text}));
                }
                AnthropicContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    let mut value = json!({"type": "thinking", "thinking": thinking});
                    if let Some(sig) = signature {
                        value["signature"] = json!(sig);
                    }
                    provider_blocks.push(value);
                }
                AnthropicContentBlock::RedactedThinking { data } => {
                    provider_blocks.push(json!({"type": "redacted_thinking", "data": data}));
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        kind: "function".to_string(),
                        function: FunctionCall {
                            name: name.clone(),
                            arguments: input.to_string(),
                        },
                    });
                    provider_blocks.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
            }
        }
        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };
        let usage = resp.usage.map(usage_from_anthropic);
        let finish_reason = if resp.stop_reason == "tool_use" {
            Some("tool_calls".to_string())
        } else {
            Some("stop".to_string())
        };
        Ok(NormalizedCompletion {
            content,
            tool_calls,
            usage,
            finish_reason,
            stop_reason: Some(resp.stop_reason),
            provider_blocks: if provider_blocks.is_empty() {
                None
            } else {
                Some(provider_blocks)
            },
        })
    }

    fn build_responses_request(&self, ctx: &ProviderResponsesContext<'_>) -> ProviderRequest {
        let instructions = ctx
            .body
            .get("instructions")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let chat_ctx = ProviderRequestContext {
            model_ref: ctx.model_ref,
            credentials: ctx.credentials,
            messages: ctx.messages,
            tools: ctx.tools,
            chat_tools: None,
            stream: ctx.stream,
            options: ctx.options,
        };
        let mut req = self.build_chat_request(&chat_ctx);
        if let Some(sys) = instructions {
            req.body["system"] = json!(sys);
        }
        req
    }

    fn parse_responses_response(
        &self,
        json: &Value,
    ) -> Result<NormalizedResponse, ProviderParseError> {
        let normalized = self.parse_chat_response(json)?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut output = Vec::new();
        if let Some(text) = normalized.content.filter(|t| !t.is_empty()) {
            output.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            }));
        }
        for tc in &normalized.tool_calls {
            if tc.kind == "function" {
                output.push(json!({
                    "type": "function_call",
                    "call_id": tc.id,
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                }));
            }
        }
        Ok(NormalizedResponse {
            id,
            output: Value::Array(output),
            usage: normalized.usage,
            provider_blocks: normalized.provider_blocks.clone(),
        })
    }

    fn supports_previous_response_id(&self) -> bool {
        false
    }

    fn supports_file_upload(&self) -> bool {
        true
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: String,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
}

fn usage_from_anthropic(u: AnthropicUsage) -> crate::proto::Usage {
    let _cache_creation = u.cache_creation_input_tokens;
    crate::usage::usage_from_breakdown(crate::usage::UsageBreakdown {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: None,
        cached_tokens: u.cache_read_input_tokens.filter(|&n| n > 0),
        reasoning_tokens: None,
    })
}

pub(crate) fn usage_from_anthropic_json(u: &Value) -> Option<crate::proto::Usage> {
    let input = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let output = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let cached = u
        .get("cache_read_input_tokens")
        .and_then(|x| x.as_u64())
        .map(|n| n as u32)
        .filter(|&n| n > 0);
    Some(crate::usage::usage_from_breakdown(
        crate::usage::UsageBreakdown {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: None,
            cached_tokens: cached,
            reasoning_tokens: None,
        },
    ))
}

fn should_cache_tools(ctx: &ProviderRequestContext<'_>) -> bool {
    ctx.tools.is_some_and(|specs| !specs.is_empty())
        && (ctx
            .options
            .system_prompt_blocks
            .as_ref()
            .is_some_and(|blocks| blocks.iter().any(|b| b.cache))
            || ctx.options.system_prompt.is_some())
}

fn build_system_payload(
    ctx: &ProviderRequestContext<'_>,
    legacy_system: Option<&str>,
) -> Option<Value> {
    if let Some(blocks) = &ctx.options.system_prompt_blocks {
        let payload = system_blocks_to_json(blocks);
        if payload.is_empty() {
            None
        } else {
            Some(Value::Array(payload))
        }
    } else if let Some(sys) = legacy_system.filter(|s| !s.is_empty()) {
        Some(Value::String(sys.to_string()))
    } else {
        ctx.options
            .system_prompt
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|sp| Value::String(sp.to_string()))
    }
}

fn system_blocks_to_json(blocks: &[SystemPromptBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter(|b| !b.text.is_empty())
        .map(|block| {
            let mut value = json!({
                "type": "text",
                "text": block.text,
            });
            if block.cache {
                value["cache_control"] = json!({"type": CACHE_CONTROL_EPHEMERAL});
            }
            value
        })
        .collect()
}

fn build_tools(specs: &[ToolSpec], cache_last: bool) -> Vec<Value> {
    let mut sorted: Vec<&ToolSpec> = specs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let last = sorted.len().saturating_sub(1);
    sorted
        .into_iter()
        .enumerate()
        .map(|(idx, s)| {
            let schema = s.parameters_schema.clone();
            let mut tool = json!({
                "name": s.name,
                "description": s.description,
                "input_schema": schema,
            });
            if cache_last && idx == last {
                tool["cache_control"] = json!({"type": CACHE_CONTROL_EPHEMERAL});
            }
            tool
        })
        .collect()
}

fn merge_assistant_tool_calls_into_blocks(
    blocks: &mut Vec<Value>,
    tool_calls: Option<&Vec<ToolCall>>,
) {
    let Some(tcs) = tool_calls else {
        return;
    };
    for tc in tcs {
        if tc.kind != "function" {
            continue;
        }
        let already_present = blocks.iter().any(|b| {
            b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                && b.get("id").and_then(|id| id.as_str()) == Some(tc.id.as_str())
        });
        if already_present {
            continue;
        }
        let input: Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| json!({}));
        blocks.push(json!({
            "type": "tool_use",
            "id": tc.id,
            "name": tc.function.name,
            "input": input,
        }));
    }
}

fn merge_assistant_text_into_blocks(blocks: &mut Vec<Value>, content: Option<&MessageContent>) {
    let Some(text) = content.and_then(|c| c.as_text()) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let has_text = blocks
        .iter()
        .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"));
    if !has_text {
        blocks.push(json!({"type": "text", "text": text}));
    }
}

#[derive(Default)]
struct PendingUserTurn<'a> {
    tool_blocks: Vec<Value>,
    user_messages: Vec<&'a ChatMessage>,
}

impl<'a> PendingUserTurn<'a> {
    fn is_empty(&self) -> bool {
        self.tool_blocks.is_empty() && self.user_messages.is_empty()
    }

    fn flush(&mut self, out: &mut Vec<Value>) {
        if self.is_empty() {
            return;
        }
        if self.tool_blocks.is_empty() && self.user_messages.len() == 1 {
            let msg = self
                .user_messages
                .pop()
                .expect("user_messages length checked");
            out.push(json!({
                "role": "user",
                "content": map_user_content(msg),
            }));
            return;
        }
        let mut blocks = std::mem::take(&mut self.tool_blocks);
        for msg in self.user_messages.drain(..) {
            append_user_blocks(&mut blocks, msg);
        }
        let content = if blocks.is_empty() {
            json!("")
        } else {
            Value::Array(blocks)
        };
        out.push(json!({
            "role": "user",
            "content": content,
        }));
    }
}

fn map_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut out = Vec::new();
    let mut pending_user = PendingUserTurn::default();

    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                if let Some(c) = &msg.content
                    && let Some(t) = c.as_text()
                {
                    system_parts.push(t.to_string());
                }
            }
            "user" => {
                pending_user.user_messages.push(msg);
            }
            "assistant" => {
                if let Some(blocks) = &msg.provider_blocks {
                    let mut content = blocks.clone();
                    merge_assistant_tool_calls_into_blocks(&mut content, msg.tool_calls.as_ref());
                    merge_assistant_text_into_blocks(&mut content, msg.content.as_ref());
                    if !content.is_empty() {
                        pending_user.flush(&mut out);
                        out.push(json!({"role": "assistant", "content": content}));
                    }
                    continue;
                }
                let mut blocks = Vec::new();
                if let Some(c) = &msg.content
                    && let Some(t) = c.as_text()
                {
                    blocks.push(json!({"type": "text", "text": t}));
                }
                if let Some(tcs) = &msg.tool_calls {
                    for tc in tcs {
                        if tc.kind == "function" {
                            let input: Value =
                                serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": input,
                            }));
                        }
                    }
                }
                if !blocks.is_empty() {
                    pending_user.flush(&mut out);
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            "tool" => {
                let content = msg.content.as_ref().and_then(|c| c.as_text()).unwrap_or("");
                pending_user.tool_blocks.push(json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id,
                    "content": content,
                }));
            }
            _ => {}
        }
    }

    pending_user.flush(&mut out);

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, out)
}

fn append_user_blocks(blocks: &mut Vec<Value>, msg: &ChatMessage) {
    match &msg.content {
        None => {}
        Some(MessageContent::Text(t)) => {
            if !t.is_empty() {
                blocks.push(json!({"type": "text", "text": t}));
            }
        }
        Some(MessageContent::Parts(parts)) => {
            blocks.extend(parts.iter().map(map_content_part));
        }
    }
}

fn map_user_content(msg: &ChatMessage) -> Value {
    match &msg.content {
        None => json!(""),
        Some(MessageContent::Text(t)) => json!(t),
        Some(MessageContent::Parts(parts)) => {
            let blocks: Vec<Value> = parts.iter().map(map_content_part).collect();
            json!(blocks)
        }
    }
}

fn map_content_part(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text { text } => json!({"type": "text", "text": text}),
        ContentPart::ImageUrl { image_url } => map_image_url(&image_url.url),
        ContentPart::File { file } => map_file_part(file),
        ContentPart::ImageRef { hash, .. } => json!({
            "type": "text",
            "text": format!("[unresolved image_ref: {hash}]")
        }),
        ContentPart::InputAudio { .. } => json!({
            "type": "text",
            "text": "[audio input not supported on anthropic adapter]"
        }),
    }
}

fn map_image_url(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((media_type, data)) = rest.split_once(";base64,")
        && !media_type.is_empty()
    {
        return json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data,
            }
        });
    }
    json!({
        "type": "image",
        "source": {
            "type": "url",
            "url": url,
        }
    })
}

fn map_file_part(file: &FileContent) -> Value {
    if let Some(data) = &file.file_data {
        let media = infer_media_type(file.filename.as_deref());
        if media == "application/pdf" {
            json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": media,
                    "data": data,
                }
            })
        } else {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media,
                    "data": data,
                }
            })
        }
    } else if let Some(id) = &file.file_id {
        json!({
            "type": "text",
            "text": format!("[file_id: {id}]")
        })
    } else {
        json!({"type": "text", "text": "[empty file part]"})
    }
}

fn infer_media_type(filename: Option<&str>) -> &'static str {
    match filename {
        Some(f) if f.ends_with(".pdf") => "application/pdf",
        Some(f) if f.ends_with(".png") => "image/png",
        Some(f) if f.ends_with(".jpg") || f.ends_with(".jpeg") => "image/jpeg",
        Some(f) if f.ends_with(".gif") => "image/gif",
        Some(f) if f.ends_with(".webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn apply_thinking_and_extra(body: &mut Value, options: &crate::chat::ChatOptions, max_tokens: u32) {
    if let Some(effort) = options.reasoning_effort.as_deref()
        && let Some(budget) =
            crate::chat::reasoning::anthropic_thinking_budget_tokens(effort, max_tokens)
    {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }
    if let Some(extra) = &options.extra_json {
        merge_extra_json(body, extra);
    }
}

fn merge_extra_json(body: &mut Value, extra: &Value) {
    if let (Value::Object(body_map), Value::Object(extra_map)) = (body, extra) {
        for (k, v) in extra_map {
            body_map.insert(k.clone(), v.clone());
        }
    }
}

/// Newer Anthropic models reject `temperature` / `top_p` / `top_k` entirely.
fn anthropic_supports_sampling_params(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    if model.contains("sonnet-5") {
        return false;
    }
    if model.contains("opus-4-7") || model.contains("opus-4-8") {
        return false;
    }
    if model.contains("fable") {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatOptions;
    use crate::openai::ChatMessage;
    use crate::providers::adapter::ProviderResponsesContext;
    use crate::providers::credentials::ProviderCredentials;
    use crate::providers::model_ref::ModelRef;

    #[test]
    fn sonnet_5_rejects_sampling_params() {
        assert!(!anthropic_supports_sampling_params("claude-sonnet-5"));
        assert!(!anthropic_supports_sampling_params(
            "anthropic:claude-sonnet-5@20260203"
        ));
    }

    #[test]
    fn older_sonnet_keeps_sampling_params() {
        assert!(anthropic_supports_sampling_params("claude-sonnet-4-6"));
        assert!(anthropic_supports_sampling_params(
            "claude-3-5-sonnet-20241022"
        ));
    }

    #[test]
    fn system_blocks_emit_cache_control() {
        let blocks = vec![
            SystemPromptBlock::cached("stable base"),
            SystemPromptBlock::uncached("dynamic tail"),
        ];
        let payload = system_blocks_to_json(&blocks);
        assert_eq!(payload.len(), 2);
        assert_eq!(
            payload[0]["cache_control"]["type"].as_str(),
            Some(CACHE_CONTROL_EPHEMERAL)
        );
        assert!(payload[1].get("cache_control").is_none());
    }

    #[test]
    fn build_request_marks_last_tool_cacheable() {
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::Anthropic, "sk-ant-test");
        let model_ref = ModelRef {
            provider: ProviderId::Anthropic,
            capability: None,
            model: "claude-sonnet-4-20250514".into(),
            raw: "anthropic:claude-sonnet-4-20250514".into(),
        };
        let options = ChatOptions {
            system_prompt_blocks: Some(vec![SystemPromptBlock::cached("base")]),
            ..Default::default()
        };
        let specs = vec![
            ToolSpec {
                name: "zebra".into(),
                description: None,
                parameters_schema: json!({"type": "object"}),
                static_tool: false,
            },
            ToolSpec {
                name: "alpha".into(),
                description: None,
                parameters_schema: json!({"type": "object"}),
                static_tool: false,
            },
        ];
        let ctx = ProviderRequestContext {
            model_ref: &model_ref,
            credentials: &creds,
            messages: &[ChatMessage::text("user", "hi")],
            tools: Some(&specs),
            chat_tools: None,
            stream: false,
            options: &options,
        };
        let req = AnthropicProvider::new().build_chat_request(&ctx);
        let tools = req.body["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"].as_str(), Some("alpha"));
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(
            tools[1]["cache_control"]["type"].as_str(),
            Some(CACHE_CONTROL_EPHEMERAL)
        );
        let system = req.body["system"].as_array().expect("system blocks");
        assert_eq!(system.len(), 1);
        assert_eq!(
            system[0]["cache_control"]["type"].as_str(),
            Some(CACHE_CONTROL_EPHEMERAL)
        );
    }

    #[test]
    fn anthropic_usage_maps_cache_reads() {
        let usage = usage_from_anthropic(AnthropicUsage {
            input_tokens: 100,
            output_tokens: 10,
            cache_creation_input_tokens: Some(80),
            cache_read_input_tokens: Some(60),
        });
        assert_eq!(usage.cached_tokens, Some(60));
        assert_eq!(usage.prompt_tokens, 100);
    }

    #[test]
    fn responses_build_hits_messages_path_and_headers() {
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::Anthropic, "sk-ant-test");
        let model_ref = ModelRef {
            provider: ProviderId::Anthropic,
            capability: None,
            model: "claude-sonnet-4-20250514".into(),
            raw: "anthropic:claude-sonnet-4-20250514".into(),
        };
        let options = ChatOptions::default();
        let body = json!({"model": "anthropic:claude-sonnet-4-20250514", "input": "hi"});
        let ctx = ProviderResponsesContext {
            model_ref: &model_ref,
            credentials: &creds,
            body: &body,
            messages: &[ChatMessage::text("user", "hi")],
            tools: None,
            stream: false,
            options: &options,
        };
        let req = AnthropicProvider::new().build_responses_request(&ctx);
        assert!(req.url.ends_with("/v1/messages"));
        assert!(
            req.headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        );
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01")
        );
    }

    #[test]
    fn responses_thinking_enabled_from_reasoning_effort() {
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::Anthropic, "sk-ant-test");
        let model_ref = ModelRef {
            provider: ProviderId::Anthropic,
            capability: None,
            model: "claude-sonnet-4-20250514".into(),
            raw: "anthropic:claude-sonnet-4-20250514".into(),
        };
        let options = ChatOptions {
            reasoning_effort: Some("medium".into()),
            max_completion_tokens: Some(8192),
            ..Default::default()
        };
        let body = json!({"model": "anthropic:claude-sonnet-4-20250514", "input": "hi"});
        let ctx = ProviderResponsesContext {
            model_ref: &model_ref,
            credentials: &creds,
            body: &body,
            messages: &[ChatMessage::text("user", "hi")],
            tools: None,
            stream: false,
            options: &options,
        };
        let req = AnthropicProvider::new().build_responses_request(&ctx);
        assert_eq!(req.body["thinking"]["type"].as_str(), Some("enabled"));
        assert!(req.body["thinking"]["budget_tokens"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn parse_responses_maps_tool_use_to_function_call() {
        let provider = AnthropicProvider::new();
        let json = json!({
            "content": [{
                "type": "tool_use",
                "id": "toolu_1",
                "name": "echo",
                "input": {"x": 1}
            }],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let normalized = provider.parse_responses_response(&json).unwrap();
        let output = normalized.output.as_array().expect("output array");
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"].as_str(), Some("function_call"));
        assert_eq!(output[0]["name"].as_str(), Some("echo"));
        assert_eq!(output[0]["call_id"].as_str(), Some("toolu_1"));
    }

    #[test]
    fn adjacent_tool_and_user_messages_coalesce_with_tool_results_first() {
        use crate::openai::ImageUrl;

        let assistant_tool_use = ChatMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![
                ToolCall {
                    id: "toolu_1".into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: "step_one".into(),
                        arguments: r#"{"a":1}"#.into(),
                    },
                },
                ToolCall {
                    id: "toolu_2".into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: "step_two".into(),
                        arguments: r#"{"b":2}"#.into(),
                    },
                },
            ]),
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: None,
        };
        let tool_one = ChatMessage {
            role: "tool".into(),
            content: Some(MessageContent::Text(r#"{"ok":true,"step":1}"#.into())),
            tool_calls: None,
            tool_call_id: Some("toolu_1".into()),
            name: Some("step_one".into()),
            refusal: None,
            provider_blocks: None,
        };
        let tool_two = ChatMessage {
            role: "tool".into(),
            content: Some(MessageContent::Text(r#"{"ok":true,"step":2}"#.into())),
            tool_calls: None,
            tool_call_id: Some("toolu_2".into()),
            name: Some("step_two".into()),
            refusal: None,
            provider_blocks: None,
        };
        let boundary_user = ChatMessage {
            role: "user".into(),
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text {
                    text: "screenshot after tools".into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "https://example.com/screen.png".into(),
                        detail: None,
                    },
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: None,
        };

        let (_, mapped) = map_messages(&[
            ChatMessage::text("user", "start"),
            assistant_tool_use,
            tool_one,
            tool_two,
            boundary_user,
        ]);

        assert_eq!(
            mapped.len(),
            3,
            "adjacent tool and user messages must coalesce into one user entry: {mapped:?}"
        );
        assert_eq!(mapped[0]["role"].as_str(), Some("user"));
        assert_eq!(mapped[1]["role"].as_str(), Some("assistant"));
        assert_eq!(mapped[2]["role"].as_str(), Some("user"));

        let blocks = mapped[2]["content"]
            .as_array()
            .expect("merged user content array");
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0]["type"].as_str(), Some("tool_result"));
        assert_eq!(blocks[0]["tool_use_id"].as_str(), Some("toolu_1"));
        assert_eq!(blocks[1]["type"].as_str(), Some("tool_result"));
        assert_eq!(blocks[1]["tool_use_id"].as_str(), Some("toolu_2"));
        assert_eq!(blocks[2]["type"].as_str(), Some("text"));
        assert_eq!(blocks[2]["text"].as_str(), Some("screenshot after tools"));
        assert_eq!(blocks[3]["type"].as_str(), Some("image"));
    }

    #[test]
    fn user_before_tool_still_orders_tool_result_first_and_coalesces_consecutive_users() {
        let assistant_tool_use = ChatMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "toolu_1".into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: "echo".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: None,
        };
        let tool_msg = ChatMessage {
            role: "tool".into(),
            content: Some(MessageContent::Text("done".into())),
            tool_calls: None,
            tool_call_id: Some("toolu_1".into()),
            name: Some("echo".into()),
            refusal: None,
            provider_blocks: None,
        };

        let (_, mapped) = map_messages(&[
            ChatMessage::text("user", "first"),
            ChatMessage::text("user", "second"),
            assistant_tool_use,
            ChatMessage::text("user", "note"),
            tool_msg,
        ]);

        assert_eq!(mapped.len(), 3);
        let first_blocks = mapped[0]["content"]
            .as_array()
            .expect("coalesced initial user blocks");
        assert_eq!(first_blocks.len(), 2);
        assert_eq!(first_blocks[0]["text"].as_str(), Some("first"));
        assert_eq!(first_blocks[1]["text"].as_str(), Some("second"));

        let last_blocks = mapped[2]["content"]
            .as_array()
            .expect("coalesced tool+user blocks");
        assert_eq!(last_blocks.len(), 2);
        assert_eq!(last_blocks[0]["type"].as_str(), Some("tool_result"));
        assert_eq!(last_blocks[0]["tool_use_id"].as_str(), Some("toolu_1"));
        assert_eq!(last_blocks[1]["type"].as_str(), Some("text"));
        assert_eq!(last_blocks[1]["text"].as_str(), Some("note"));
    }

    #[test]
    fn user_image_data_url_maps_to_base64_and_http_urls_keep_url_source() {
        use crate::openai::ImageUrl;

        let msg = ChatMessage {
            role: "user".into(),
            content: Some(MessageContent::Parts(vec![
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "data:image/png;base64,aW1hZ2U=".into(),
                        detail: None,
                    },
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "https://example.com/photo.jpg".into(),
                        detail: None,
                    },
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "http://example.com/plain.png".into(),
                        detail: None,
                    },
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: None,
        };

        let content = map_user_content(&msg);
        let blocks = content.as_array().expect("multipart blocks");
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[0],
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "aW1hZ2U=",
                }
            })
        );
        assert_eq!(
            blocks[1],
            json!({
                "type": "image",
                "source": {
                    "type": "url",
                    "url": "https://example.com/photo.jpg",
                }
            })
        );
        assert_eq!(
            blocks[2],
            json!({
                "type": "image",
                "source": {
                    "type": "url",
                    "url": "http://example.com/plain.png",
                }
            })
        );
    }
}
