# Superglue packages

The Rust core (repo root) and all language bindings are developed **together** and share a single version.

| Package | Path |
|---------|------|
| Core | `.` (`superglue` crate) |
| Python | `superglue-py/` |
| JavaScript (napi-rs) | `superglue-js/` |
| Kotlin (JNI) | `superglue-kt/` |

## Version

**Source of truth:** `[workspace.package].version` in [`Cargo.toml`](Cargo.toml).

When bumping the release version, edit that field only, then run:

```bash
./scripts/sync-superglue-versions.sh
./scripts/check-superglue-version-parity.sh
```

## Build (Rust workspace)

```bash
cargo build
cargo test --features mcp
```

## Publish

Cut a GitHub Release on tag `v` plus the workspace version. Example: workspace `1.0.0` uses tag `v1.0.0`.

That release starts three workflows:

| Workflow | Registry | Download if the registry upload does not run |
| --- | --- | --- |
| `publish-pypi.yml` | PyPI project `superglue-py` | Wheels and the source distribution on the GitHub release |
| `publish-npm.yml` | npm package `superglue-js` plus one package per target | `.node` files on the GitHub release |
| `publish-maven.yml` | Maven Central `com.superglue.kt:superglue-android` | `superglue-android-<version>.aar` on the GitHub release |

The Rust crate is not published to crates.io. Depend on this git repository.

Only a published release, or a manual run with **publish** set, uploads to a registry. Pull requests do not publish. A manual publish must use the tag ref `v<version>`. A manual run from a branch only builds artifacts.

### One-time setup

1. On PyPI, add a pending publisher for project `superglue-py`. Set the owner to `pinhole-io`, the repository to `superglue`, the workflow to `publish-pypi.yml`, and the environment to `pypi`.
2. Create GitHub environments named `pypi`, `npm`, and `maven`. Add required reviewers when you want a person to approve each upload.
3. Put an npm automation token in secret `NPM_TOKEN` on the `npm` environment. The token must be allowed to publish `superglue-js` and the platform names `superglue-js-linux-x64-gnu`, `superglue-js-linux-arm64-gnu`, `superglue-js-darwin-x64`, and `superglue-js-darwin-arm64`.
4. On [central.sonatype.com](https://central.sonatype.com), verify the namespace `com.superglue.kt`. Put the user token in `CENTRAL_TOKEN`. Put the ASCII-armored GPG private key in `GPG_PRIVATE_KEY`. Put the key passphrase in `GPG_PASSPHRASE`. Store those three secrets on the `maven` environment.

### What a failure can expose

The workflows read those secrets at publish time. They do not print the secret values. A person who can publish a release on this repository can publish these packages. Protect the `pypi`, `npm`, and `maven` environments when that risk is too open.
