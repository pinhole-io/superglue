# SuperGlue gateway admin

React SPA for operators of the SuperGlue LLM gateway.

## Build

1. Run `npm ci`.
2. Run `npm run build`.
3. Build SuperGlue with `cargo build --features gateway`.
4. Open `/admin/` on the gateway.

The gateway embeds `dist/` into the binary. The checked-in `dist/index.html`
is a build hint for source checkouts. Run the build before deployment.

## Auth

The UI keeps the gateway master key in `sessionStorage` and sends it to the
same-origin `/v1/*` admin API as `X-Superglue-Key`.

## First-run setup

After sign-in, if the gateway has no users, keys, or budgets, the UI opens a
required setup wizard. It creates a default monthly budget, one user, and one
virtual key with `openai:*` and `anthropic:*`. The plaintext key is shown once.

## Routes

| Path | Page |
|------|------|
| `/admin/` | Overview |
| `/admin/users` | Users |
| `/admin/keys` | Keys |
| `/admin/budgets` | Budgets |
| `/admin/usage` | Usage |
| `/admin/providers` | Providers |
| `/admin/capture` | Capture status |
| `/admin/capture/records` | Capture records |
| `/admin/capture/records/:requestId` | Capture record detail |
| `/admin/styleguide` | Style guide |

## Model families

Key allowlists group models by family:

- **System 1** — `typesafe:*` evaluation models via `POST /v1/systemone`
- **System 2** — chat and Responses providers via `/v1/chat/completions` and `/v1/responses`
- **Embeddings** — `provider:embedding:…` models via `POST /v1/embeddings`

## Dev

```bash
npm run dev
```

Vite serves on port `5174` and proxies `/v1` and `/health` to
`VITE_GATEWAY_URL` or `http://127.0.0.1:8082`.
