# Multi-provider routing and file attachments

Chat models use a `provider:model` string.
Example: `openai:gpt-4o-mini` or `anthropic:claude-sonnet-4-20250514`.
The prefix selects the key and the base URL.
The suffix is the model id sent to that provider.
A bare model id uses OpenAI.
An unknown prefix also falls back to OpenAI.
A slash form logs a deprecation warning.

## API keys

`ProviderCredentials::from_env` loads one key variable per provider.

| Provider | Key variable | Also accepted |
| --- | --- | --- |
| OpenAI | `OPENAI_API_KEY` | |
| Anthropic | `ANTHROPIC_API_KEY` | |
| xAI | `XAI_API_KEY` | |
| Groq | `GROQ_API_KEY` | |
| OpenRouter | `OPENROUTER_API_KEY` | `OPENROUTER_GATEWAY_KEY` when the primary variable is unset |
| RunInfra | `RUNINFRA_GATEWAY_KEY` | |
| Vercel AI Gateway | `VERCEL_GATEWAY_KEY` | |
| Amazon Bedrock | `AWS_BEARER_TOKEN_BEDROCK` | `BEDROCK_API_KEY` when the primary variable is unset |
| TypeSafe | `TYPESAFE_API_KEY` | |

Override a key with `ClientBuilder::api_key_for` in Rust.
Python, JavaScript, and Kotlin use `api_keys` / `apiKeys`.

## Base URLs

The environment loader reads three base URL variables:

| Variable | Provider |
| --- | --- |
| `OPENAI_BASE_URL` | OpenAI. Use this for Ollama or another OpenAI-compatible host. |
| `TYPESAFE_BASE_URL` | TypeSafe |
| `BEDROCK_BASE_URL` | Amazon Bedrock |

When `BEDROCK_BASE_URL` is unset, `AWS_REGION` or `AWS_DEFAULT_REGION` selects the runtime host.
The default Region is `us-east-1`.
The Region value must look like `us-east-1` or `us-gov-west-1`.

Other providers use the built-in host until you call `ClientBuilder::base_url_for`.
The matching binding option has the same effect.
Leave the `/v1` suffix off the base URL. The client appends the path.

| Provider | Default base URL |
| --- | --- |
| OpenAI | `https://api.openai.com` |
| Anthropic | `https://api.anthropic.com` |
| xAI | `https://api.x.ai` |
| Groq | `https://api.groq.com/openai` |
| OpenRouter | `https://openrouter.ai/api` |
| RunInfra | `https://api.runinfra.ai` |
| Vercel | `https://ai-gateway.vercel.sh` |
| Amazon Bedrock | `https://bedrock-runtime.us-east-1.amazonaws.com/openai` |
| TypeSafe | `https://api.typesafe.ai` |

RunInfra chat requests send `X-Client-Request-Id`.
RunInfra uses `max_tokens` on the wire.

## Amazon Bedrock

The client sends `Authorization: Bearer`.
A model id can contain a colon. Only the first colon selects the provider:

```text
bedrock:openai.gpt-oss-120b-1:0
bedrock:us.anthropic.claude-sonnet-4-5-20250929-v1:0
```

The gateway lists models with `ListFoundationModels` and `ListInferenceProfiles`.
The Files API is not on this route. Put image bytes in the chat message.

## TypeSafe

TypeSafe is the System One evaluation API.
It is not a chat provider.
Call `Client::system_one` or gateway `POST /v1/systemone`.
Use `typesafe:jev-latest` or a bare `jev-latest` on that route.
Do not send `typesafe:...` to `/v1/chat/completions` or `/v1/responses`.

Rust example: `cargo run --example 37_systemone`.

## Per-key rate limits

Pass `requests_per_second_for` (Python) or `requestsPerSecondFor` (JavaScript and Kotlin).
The map key is the provider name. The value is requests per second.
The legacy `requests_per_second` / `quota_per_second` applies to OpenAI when the map is empty.

## Embeddings

Use `Client::embed` or `Client::embed_many` with `provider:model`.
OpenAI-compatible providers call `/v1/embeddings`.
Amazon Bedrock calls `InvokeModel`.
Anthropic and TypeSafe do not embed.

Use `provider:embedding:model` when the model must appear in an embedding catalog.
Example: `openai:embedding:text-embedding-3-small`.
Bedrock text embeddings use the same tag, for example `bedrock:embedding:amazon.titan-embed-text-v2:0`.

Titan text models and Cohere Embed models use `InvokeModel`.
Titan accepts one string per request, up to 32 strings.
Cohere accepts one batch of up to 96 strings.
Image embedding models are rejected.

The tag is authoritative.
The gateway strips `embedding:` before a direct provider request.
It keeps the full reference for gateway-routed requests.
Add each tagged model to the key allowlist.
Superglue does not infer this capability from a model name.

## Realtime voice

Enable the `realtime` feature.
The first adapter is xAI Speech to Speech:

```text
xai:grok-voice-think-fast-2.0
```

`VoiceSession` uses the xAI realtime WebSocket and requires `XAI_API_KEY`.
Keep that key on a trusted server when a browser or phone connects.

```bash
cargo run --example 36_realtime_voice --features realtime
```

## File transcription

`audio::transcribe` sends file bytes to `/v1/audio/transcriptions`.
OpenAI uses `whisper-1`. Groq uses `whisper-large-v3`.
Other providers return an error.
The size cap is 32 MB.
Do not log the audio bytes or the transcript.

The gateway exposes the same call at `POST /v1/audio/transcriptions` and `POST /v1/speech/transcriptions`.
A non-default base URL uses `/v1/speech/transcriptions` so a gateway hop does not recurse.

## File uploads

Two patterns:

1. **Inline bytes.** Build a user message with `message_with_file_bytes` and pass it to `complete_messages`.
2. **Provider file API.** `upload_file` posts to the provider files endpoint and returns a `file_id`.

`max_upload_bytes` caps an inline attachment. The default is 20 MB.

Rust conversations can store that message with `ClientConversation::push_user_message`.
The method sets the role to `user`.
Python and JavaScript still append text with `push_user`.

## Streaming with tools

When tools are registered, `stream` runs the same multi-round loop as `complete`.
Token deltas arrive between rounds.
Kotlin, Python, and JavaScript use that path when the registry is not empty.

On a Rust `ClientConversation`, an OpenAI stream uses the Responses API.
Other providers use chat completions.

## Self-hosted proxy providers

A gateway operator can register an OpenAI-compatible server under its own prefix.
Callers then use `slug:model`, for example `homelab:llama3.1`.

The slug must not match a builtin provider.
Builtin prefixes are `openai`, `anthropic`, `xai`, `groq`, `openrouter`, `runinfra`, `vercel`, `bedrock`, and `typesafe`.

Set the base URL without a `/v1` suffix. The gateway appends these paths:

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/models`

`homelab:org/model:tag` sends `org/model:tag` upstream.
`homelab:embedding:bge-m3` sends `bge-m3` to the embeddings path.

Only the master key can create or change a proxy.
The upstream API key is optional.
The gateway stores it encrypted with the master key.
Responses show the last four characters.

Allow a key to call the proxy with `homelab:*` or a literal model id.
An unknown slug returns 400. A disabled proxy returns 403.

This registry is gateway state. The in-process client does not load it.

## Examples

- Rust: `cargo run --example 29_multi_provider`
- Python: `python examples/29_multi_provider.py` from `superglue-py/`
- JavaScript: `node examples/29_multi_provider.mjs` from `superglue-js/`
- Kotlin: `FeatureExamplesTest.example29_multi_provider`
