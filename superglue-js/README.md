# superglue-js

Node.js bindings for the Rust core through [napi-rs](https://napi.rs/).

The surface matches [superglue-py](../superglue-py): `Client`, tools, hooks, guardrails, streaming, batch, conversations, and agents.
`package.json` uses `[workspace.package].version` from [`../Cargo.toml`](../Cargo.toml).
After a version bump, run `../scripts/sync-superglue-versions.sh`.

## Install

```bash
npm install superglue-js
```

Prebuilt binaries cover Linux x86_64, Linux aarch64, macOS x86_64, and macOS Apple Silicon.
On another platform, build from this repository.
Each GitHub release also stores the `.node` files.

## Requirements

- Node.js 18 or newer
- A Rust toolchain when you build from this repository

## Build

From `superglue-js/`:

```bash
npm install
npm run build
```

- `npm run build:native` builds the release addon.
- `npm run build:native:debug` builds the debug addon.
- `npm run build:ts` builds the TypeScript facade (`dist/facade.js`).

The binding crate enables the `code` and `mcp` features of the Rust core.

## Facade

Prefer `superglue-js/facade` for option objects, tools, and guardrails.

```js
const { createClient, registerTool } = require("superglue-js/facade");

async function main() {
  const client = createClient({
    apiKey: process.env.OPENAI_API_KEY,
    model: "openai:gpt-4o-mini",
  });
  await registerTool(client, {
    name: "get_weather",
    description: "Get weather for a city.",
    parameters: {
      type: "object",
      properties: { location: { type: "string" } },
      required: ["location"],
    },
    execute: ({ location }) => ({ temp: 22, unit: "C", location }),
  });
  const out = await client.complete("What is the weather in Paris?");
  console.log(out.content);
}

main();
```

Omit `model` to use `gpt-5.4-nano-2026-03-17-mini`.

## Native addon

`require("superglue-js")` loads the napi class.
Later constructor arguments are optional. See `index.d.ts`.

```js
const { Client, version } = require("superglue-js");

console.log(version());
const client = new Client(process.env.OPENAI_API_KEY, "openai:gpt-4o-mini");
const outcome = await client.complete("Say hello in one short sentence.");
console.log(outcome.content);
```

## Guardrails

Use `registerGuardrail(client, (stage, content) => …)` from the facade.
The raw `client.registerGuardrail` callback receives `(err, stage, content)` or `(err, [stage, content])`.
The facade accepts both shapes.
A plain `(stage, content)` function on `client.registerGuardrail` binds the wrong parameters.

Handlers must be synchronous.
Return a string, or `throw` to block.
An `async` handler is not awaited. A `throw` inside it can become an unhandled rejection.

## Tools

Pass an explicit JSON Schema in `parameters`.
The callback may be sync or async and must return a JSON object.
There is no Python-style introspection.

## Providers and files

Chat models use `provider:model`.
Set `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and the other provider variables, or pass `apiKeys`.
Per-provider rate limits use `requestsPerSecondFor`.
Files use `client.uploadFile(path)` and `messageWithFileBytes` on the facade.
When tools are registered, `stream()` runs the tool loop.
Details: [`../docs/multi-provider.md`](../docs/multi-provider.md).

## Voice

`client.voiceConnect(config, onEvent)` opens a realtime voice session on OpenAI or xAI.
`onEvent` receives `{ event, audio }`. `audio` is a `Buffer` on `output_audio` events.
The config never holds an API key. The session uses the client key.
Example: [`examples/36_realtime_voice.mjs`](examples/36_realtime_voice.mjs).

## Examples

```bash
npm run build
export OPENAI_API_KEY=sk-...
node examples/01_simple_completion.mjs
```

The parity table is in [`../examples/README.md`](../examples/README.md).
Notes for this package: [`examples/README.md`](examples/README.md).

## CI

Test workflow: [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml).

This package lives in this repository next to the Rust crate.
A separate git submodule is required only if you split the package out later.
