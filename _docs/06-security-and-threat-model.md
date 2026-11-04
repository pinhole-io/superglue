# Security and threat model

This note tiers expectations.
A stronger tier is not the default product requirement.

## What the code does today

- The license is MIT. The source ships with the binary.
- The release profile enables LTO and strips symbols.
- Provider API keys use `secrecy::SecretString` and `zeroize` on drop.
- Logs default to `ScrubMode::Redact`. See [Observability](05-observability.md).
- Gateway virtual keys are stored as SHA-256 hashes. Compare uses a constant-time check.
- A proxy provider API key is encrypted with the gateway master key.
- TLS for provider calls is rustls through `reqwest`.

## Warning

A prompt that you send to a model provider leaves this process.
Memory hardening does not cover that hop.
The `prometheus` feature listens on `0.0.0.0:9090/metrics` when `init_metrics` runs.
Do not expose that port on a public host.

## Tier A: Normal commercial packaging

Typical goals: ship updates safely and use standard transport security.

- Prefer a documented wire protocol, such as gRPC with protobuf, when the caller is another process.
- Use TLS for remote traffic. Prefer mTLS when the peer must be authenticated.
- Keep a dependency inventory (`cargo auditable` or an SBOM) for supply-chain review.
- License servers and entitlement checks are product choices. They are not part of this crate.

## Tier B: Strong local confidentiality

Goals: keep prompts and tool output out of disk, crash dumps, and casual logs.

- Keep workflow and audit state in memory when policy allows it. Use `MemoryRunStore` for that.
- `JsonlRunStore` writes prompts, tool arguments, tool output, and model replies to disk in plain text.
  - It creates the directory with mode `0700` and each file with mode `0600`.
  - It does not write API keys or headers. Request options are stored without credentials.
  - It accepts only request ids that match `[A-Za-z0-9_-]+`, so an id cannot leave the directory.
  - Put the directory on an encrypted volume. Delete old files under your retention policy.
- `zeroize` and `secrecy` cover API keys. Apply the same pattern to any new secret buffer.
- `mlock` can reduce swap risk for a specific page. It does not stop every attack.
- Use rustls and short-lived certificates for a remote control plane.

Data sent to an external model API leaves the trust boundary.
Treat provider calls and logs as the main exfiltration paths.

## Tier C: Research topics

Self-destruct flows, binary overwrite, and remote heartbeats are exploratory.
They are not implemented.

### Tamper response

Ideas include overwriting the executable, zeroing state, or aborting on an integrity failure.

Some operating systems lock the running executable. A design must use a launcher or delayed cleanup.
A copied binary remains usable after local deletion.

### Encrypted loader

Encryption at rest, with a key delivered after authentication, is a distribution design.
It needs key management. It is not a language feature of this crate.

## Observability as a leak

The usual leaks are:

- Provider API traffic
- Tool output that contains secrets
- Traces that store full prompts

Tool arguments and `tool` role text stay redacted on LLM spans in every scrub mode.
`Allow` still writes other message text. See [Observability](05-observability.md).

## Limits of userspace

A caller with kernel access, hypervisor access, or a disk image can recover data after zeroization.
State claims at the tier you actually implement.

## See also

- [Network privacy and telemetry shaping](07-network-privacy-and-telemetry-shaping.md)
- [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md)
- [LLM gateway](12-llm-gateway.md)
