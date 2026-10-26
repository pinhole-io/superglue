# Multi-provider routing and file attachments

Superglue chat models use a `provider:model` string (for example `openai:gpt-4o-mini` or `anthropic:claude-sonnet-4-20250514`). The provider prefix selects which API credentials and base URL are used; the model suffix is sent to that provider's API.

## Environment variables

| Provider | API key env | Base URL env (optional) |
|----------|-------------|-------------------------|
| OpenAI | `OPENAI_API_KEY` | `OPENAI_BASE_URL` |
| Anthropic | `ANTHROPIC_API_KEY` | `ANTHROPIC_BASE_URL` |
| xAI | `XAI_API_KEY` | `XAI_BASE_URL` |
| Groq | `GROQ_API_KEY` | `GROQ_BASE_URL` |
| OpenRouter | `OPENROUTER_API_KEY` | `OPENROUTER_BASE_URL` |
| RunInfra | `RUNINFRA_GATEWAY_KEY` | `RUNINFRA_BASE_URL` |
| Vercel AI Gateway | `VERCEL_GATEWAY_KEY` | `VERCEL_BASE_URL` |
| Amazon Bedrock | `AWS_BEARER_TOKEN_BEDROCK` | `BEDROCK_BASE_URL` |

Bindings load these via `ProviderCredentials::from_env()` when constructing a client. You can override per provider with `api_keys` (Python/JS/Kotlin) or `ClientBuilder` in Rust.

## Amazon Bedrock

Use a Bedrock API key. `BEDROCK_API_KEY` is accepted when `AWS_BEARER_TOKEN_BEDROCK` is unset. The client sends `Authorization: Bearer`.

The default base URL is `https://bedrock-runtime.us-east-1.amazonaws.com/openai`. Set `AWS_REGION` or `AWS_DEFAULT_REGION` to change the Region. `BEDROCK_BASE_URL` overrides both. Leave the `/v1` suffix off the base URL. The client appends `/v1/chat/completions` and `/v1/responses`.

A model id can contain a colon. Only the first colon selects the provider:

```text
bedrock:openai.gpt-oss-120b-1:0
bedrock:us.anthropic.claude-sonnet-4-5-20250929-v1:0
```

The gateway lists models with `ListFoundationModels` and `ListInferenceProfiles`. An embedding model is `bedrock:embedding:{modelId}`.

```text
bedrock:embedding:amazon.titan-embed-text-v2:0
bedrock:embedding:cohere.embed-english-v3
```

Titan text models and Cohere Embed models use `InvokeModel`. Titan accepts one string per request, up to 32 strings. Cohere accepts one batch of up to 96 strings. Image embedding models are rejected.

The Files API is not on this route. Put image bytes in the chat message.

## Per-key rate limits

Pass `requests_per_second_for` (Python) / `requestsPerSecondFor` (JS/Kotlin) as a map from provider name to QPS. The legacy single `requests_per_second` / `quota_per_second` applies to OpenAI when no per-provider map is set.

## Embeddings

Rust callers can use `Client::embed` or `Client::embed_many` with a
`provider:model` string. OpenAI-compatible providers use `/v1/embeddings`.

The gateway exposes the same shape at `POST /v1/embeddings`. Anthropic and
TypeSafe do not support this capability.

Amazon Bedrock embeds text with `InvokeModel`. Use
`bedrock:embedding:amazon.titan-embed-text-v2:0` or a Cohere Embed model id.

Use `provider:embedding:model` for a model that must appear in an embedding
catalog. For example, use `openai:embedding:text-embedding-3-small`.

The tag is authoritative. The gateway strips `embedding:` before a direct
provider request. It preserves the complete reference for gateway requests.
Gateway administrators must add each tagged model to the key's model list.
Superglue does not infer this capability from a provider or model name.

## Realtime voice

Enable the `realtime` feature for duplex voice sessions. The first adapter is
xAI Speech to Speech:

```text
xai:grok-voice-think-fast-2.0
```

`VoiceSession` uses the xAI realtime WebSocket and requires `XAI_API_KEY`.
Keep that key on a trusted server when a browser or mobile client connects.
The applet integration proxies audio through `harn-server`.

## File uploads

Two patterns:

1. **Inline bytes** — build a user message with `message_with_file_bytes` / `messageWithFileBytes` / `messageWithFileBytes` (Kotlin) and pass it to `complete_messages`.
2. **Provider file API** — `upload_file` posts to the provider's files endpoint and returns a `file_id` for use in chat messages.

Set `max_upload_bytes` to cap inline attachment size (default 20 MB).

## Streaming with tools

When tools are registered on the client, `stream()` runs the same multi-round tool loop as `complete()`, emitting token deltas between rounds. Kotlin, Python, and JS bindings all use `stream_complete_with_tools` when the tool registry is non-empty.

## Self-hosted proxy providers

A gateway operator can register an OpenAI-compatible server under its own prefix.
Callers then use `slug:model`, for example `homelab:llama3.1`.

The slug must not match a builtin provider. Builtin providers stay `openai`, `anthropic`, `xai`, `groq`, `openrouter`, `runinfra`, `vercel`, `bedrock`, and `typesafe`.

Set the base URL without a `/v1` suffix. The gateway appends these paths:

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/models`

`homelab:org/model:tag` sends `org/model:tag` upstream.
`homelab:embedding:bge-m3` sends `bge-m3` to the embeddings path.

Only the master key can create or change a proxy. The upstream API key is optional. The gateway stores it encrypted with the master key. Responses show the last four characters.

Allow a key to call the proxy with `homelab:*` or a literal model id.
An unknown slug returns 400. A disabled proxy returns 403.

This registry is gateway state. The in-process client does not load it.

## Examples

- Rust: `cargo run --example 29_multi_provider` (from `projects/superglue/`)
- Python: `python examples/29_multi_provider.py`
- JS: `node examples/29_multi_provider.mjs`
- Kotlin: `FeatureExamplesTest.example29_multi_provider`
