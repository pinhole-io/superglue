# Superglue monorepo

| Component | Path | Role |
| --- | --- | --- |
| superglue | repository root | Rust library and CLI |
| superglue-js | `superglue-js/` | Node.js napi-rs binding |
| superglue-py | `superglue-py/` | Python PyO3 binding |
| superglue-kt | `superglue-kt/` | Kotlin and Android JNI binding |
| gateway-admin | `gateway-admin/` | Admin UI embedded by the `gateway` feature |
| deploy | `deploy/` | Gateway host scripts |
| docs | `docs/` and `_docs/` | Provider guide and design notes |

Version sync and publish notes: [PACKAGES.md](PACKAGES.md).

When this repository is checked out as `projects/superglue`, sibling crates use `path = "../superglue"`.
