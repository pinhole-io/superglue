#!/usr/bin/env node
/** Smoke tests + execute every examples/*.mjs. */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { Client, version } = require("..");

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const examplesDir = join(root, "examples");

const SKIP_MARKERS = [
  "Set OPENAI_API_KEY",
  "OPENAI_API_KEY is not set",
  "OPENAI_API_KEY not set",
];

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

function exampleOk(script, result) {
  if (result.status === 0) return true;
  if (process.env.OPENAI_API_KEY) return false;
  const combined = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  return result.status === 1 && SKIP_MARKERS.some((m) => combined.includes(m));
}

function smokeTest() {
  assert(typeof version() === "string" && version().length > 0, "version()");

  const client = new Client("sk-test", "gpt-5.4-nano-2026-03-17-mini");
  assert(client.toolsRegistryPtr() > 0, "toolsRegistryPtr");
  assert(client.hooksRegistryPtr() > 0, "hooksRegistryPtr");
  console.log("smoke: ok");
}

/** Accept one WebSocket handshake on 127.0.0.1, then read until the peer leaves. */
function localWebSocket() {
  return new Promise((resolve) => {
    const server = createServer((socket) => {
      let request = "";
      socket.on("data", (chunk) => {
        if (request.includes("\r\n\r\n")) return;
        request += chunk.toString("latin1");
        if (!request.includes("\r\n\r\n")) return;
        const key = /sec-websocket-key:\s*(\S+)/i.exec(request)[1];
        const accept = createHash("sha1")
          .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
          .digest("base64");
        socket.write(
          "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n" +
            `Connection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`,
        );
      });
      socket.on("error", () => {});
    });
    server.listen(0, "127.0.0.1", () =>
      resolve({ server, url: `ws://127.0.0.1:${server.address().port}` }),
    );
  });
}

async function rejects(promise, pattern, msg) {
  try {
    await promise;
  } catch (error) {
    assert(pattern.test(String(error.message)), `${msg}: ${error.message}`);
    return error;
  }
  assert(false, `${msg}: expected a rejection`);
}

async function voiceTests() {
  const client = new Client("sk-test", "gpt-4o-mini");
  const ignore = () => {};
  await rejects(
    client.voiceConnect({ provider: "anthropic" }, ignore),
    /supported provider/,
    "voice rejects unsupported provider",
  );
  await rejects(
    client.voiceConnect({ provider: "openai" }, ignore),
    /model/,
    "voice requires a realtime model for OpenAI",
  );
  const leak = await rejects(
    client.voiceConnect({ provider: "openai", model: "gpt-realtime", api_key: "sk-leak" }, ignore),
    /api_key/,
    "voice rejects an api_key field",
  );
  assert(!leak.message.includes("sk-leak"), "voice config error hides the key value");

  const { server, url } = await localWebSocket();
  const events = [];
  let sawClosed;
  const closed = new Promise((resolve) => (sawClosed = resolve));
  const session = await client.voiceConnect(
    { provider: "openai", model: "gpt-realtime", base_url: url },
    (err, payload) => {
      if (err) return;
      events.push(payload.event);
      if (payload.event.type === "closed") sawClosed();
    },
  );
  await session.sendAudio(Buffer.from([0, 0]));
  await session.close();
  await closed;
  const deadline = Date.now() + 5000;
  for (;;) {
    try {
      await session.sendText("too late");
    } catch (error) {
      assert(/closed/.test(error.message), `closed session error: ${error.message}`);
      break;
    }
    assert(Date.now() < deadline, "closed session kept accepting commands");
    await new Promise((r) => setTimeout(r, 10));
  }
  assert(events[0].type === "session_created", "first voice event is session_created");
  server.close();
  console.log("voice: ok");
}

function runExamples() {
  const scripts = readdirSync(examplesDir)
    .filter((f) => f.endsWith(".mjs"))
    .sort();
  assert(scripts.length > 0, "examples exist");

  for (const script of scripts) {
    const path = join(examplesDir, script);
    const result = spawnSync(process.execPath, [path], {
      cwd: root,
      encoding: "utf8",
      env: process.env,
    });
    if (!exampleOk(script, result)) {
      console.error(`example ${script} failed (exit ${result.status})`);
      console.error(result.stdout);
      console.error(result.stderr);
      process.exit(1);
    }
    console.log(`example ${script}: ok`);
  }
}

smokeTest();
await voiceTests();
runExamples();
console.log("all tests passed");
