//! Parse one assistant turn as a JSON object and check it against a schema.
//!
//! A bad schema fails before any provider call. A bad object retries once.

use serde_json::Value;

use crate::chat::{ChatError, ChatOptions, CompletionOutcome, complete_with_tools};
use crate::guardrails::GuardrailRegistry;
use crate::hooks::HookRegistry;
use crate::http::HttpClient;
use crate::openai::{ChatMessage, JsonSchemaFormat, ResponseFormat};
use crate::tools::ToolRegistry;

const SCHEMA_MAX_BYTES: usize = 64 * 1024;
const PATH_MAX_CHARS: usize = 200;

/// The JSON value and the completion that produced it.
#[derive(Debug, Clone)]
pub struct ObjectOutcome {
    pub value: Value,
    pub outcome: CompletionOutcome,
}

struct PreparedSchema {
    format: JsonSchemaFormat,
    validator: jsonschema::Validator,
}

struct Miss {
    kind: String,
    note: String,
}

impl Miss {
    fn json() -> Self {
        Self {
            kind: "json".to_string(),
            note: "The JSON failed to parse. Return only a JSON object that matches the schema."
                .to_string(),
        }
    }

    fn schema(path: String) -> Self {
        let path = clip_path(&path);
        if path.is_empty() {
            return Self {
                kind: "schema".to_string(),
                note: "The JSON does not match the schema. Return only a JSON object that matches the schema.".to_string(),
            };
        }
        Self {
            kind: format!("schema: {path}"),
            note: format!(
                "The JSON does not match the schema at {path}. Return only a JSON object that matches the schema."
            ),
        }
    }
}

/// Complete `user_message` and return one object that matches `schema`.
///
/// This sets `response_format` for this call. It does not change `options`.
pub async fn complete_object(
    http: &HttpClient,
    registry: &ToolRegistry,
    hooks: &HookRegistry,
    guardrails: &GuardrailRegistry,
    user_message: &str,
    schema: &JsonSchemaFormat,
    options: &ChatOptions,
) -> Result<ObjectOutcome, ChatError> {
    let prepared = prepare_schema(schema)?;
    let options = options_for_schema(options, &prepared.format);
    let messages = vec![ChatMessage::text("user", user_message)];
    let first = complete_with_tools(http, registry, hooks, guardrails, messages, &options).await?;
    match check_outcome(&prepared.validator, &first) {
        Ok(value) => Ok(ObjectOutcome {
            value,
            outcome: first,
        }),
        Err(miss) => {
            tracing::info!(kind = %miss.kind, "structured output missed");
            let mut messages = first.messages;
            messages.push(ChatMessage::text("user", miss.note));
            let second =
                complete_with_tools(http, registry, hooks, guardrails, messages, &options).await?;
            match check_outcome(&prepared.validator, &second) {
                Ok(value) => Ok(ObjectOutcome {
                    value,
                    outcome: second,
                }),
                Err(miss) => {
                    tracing::info!(kind = %miss.kind, "structured output missed");
                    Err(ChatError::Object(miss.kind))
                }
            }
        }
    }
}

fn prepare_schema(schema: &JsonSchemaFormat) -> Result<PreparedSchema, ChatError> {
    let bytes = serde_json::to_vec(&schema.schema)
        .map_err(|_| ChatError::Object("schema document".to_string()))?;
    if bytes.len() > SCHEMA_MAX_BYTES {
        return Err(ChatError::Object("schema document".to_string()));
    }
    let validator = jsonschema::validator_for(&schema.schema)
        .map_err(|_| ChatError::Object("schema document".to_string()))?;
    let mut format = schema.clone();
    format.strict = Some(true);
    Ok(PreparedSchema { format, validator })
}

fn options_for_schema(options: &ChatOptions, schema: &JsonSchemaFormat) -> ChatOptions {
    let mut options = options.clone();
    options.response_format = Some(ResponseFormat::JsonSchema {
        json_schema: schema.clone(),
    });
    options
}

fn check_outcome(
    validator: &jsonschema::Validator,
    outcome: &CompletionOutcome,
) -> Result<Value, Miss> {
    let text = outcome.content.as_deref().unwrap_or("");
    let value = parse_assistant_json(text)?;
    let Some(error) = validator.iter_errors(&value).next() else {
        return Ok(value);
    };
    Err(Miss::schema(error.instance_path().to_string()))
}

fn parse_assistant_json(text: &str) -> Result<Value, Miss> {
    let stripped = strip_one_fence(text);
    serde_json::from_str(&stripped).map_err(|_| Miss::json())
}

fn strip_one_fence(text: &str) -> String {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let rest = if let Some(rest) = rest.strip_prefix('\n') {
        rest
    } else if let Some(newline) = rest.find('\n') {
        let tag = &rest[..newline];
        if tag
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '+')
        {
            &rest[newline + 1..]
        } else {
            return trimmed.to_string();
        }
    } else {
        return trimmed.to_string();
    };
    let Some(body) = rest.trim_end().strip_suffix("```") else {
        return trimmed.to_string();
    };
    body.trim().to_string()
}

fn clip_path(path: &str) -> String {
    if path.len() <= PATH_MAX_CHARS {
        return path.to_string();
    }
    let mut end = PATH_MAX_CHARS;
    while end > 0 && !path.is_char_boundary(end) {
        end -= 1;
    }
    path[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_one_json_fence() {
        let text = "```json\n{\"name\":\"ada\"}\n```";
        assert_eq!(strip_one_fence(text), "{\"name\":\"ada\"}");
    }

    #[test]
    fn leaves_plain_json() {
        assert_eq!(strip_one_fence("{\"name\":\"ada\"}"), "{\"name\":\"ada\"}");
    }

    #[test]
    fn rejects_a_schema_over_64_kib() {
        let schema = JsonSchemaFormat {
            name: "object".into(),
            description: None,
            schema: json!({ "type": "object", "description": "x".repeat(SCHEMA_MAX_BYTES) }),
            strict: None,
        };
        let err = prepare_schema(&schema).err().expect("size");
        assert!(matches!(err, ChatError::Object(kind) if kind == "schema document"));
    }

    #[test]
    fn rejects_a_schema_that_does_not_compile() {
        let schema = JsonSchemaFormat {
            name: "object".into(),
            description: None,
            schema: json!({ "type": "not-a-json-schema-type" }),
            strict: None,
        };
        let err = prepare_schema(&schema).err().expect("compile");
        assert!(matches!(err, ChatError::Object(kind) if kind == "schema document"));
    }
}
