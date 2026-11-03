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

## Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on push to `main` and on every pull request.

| Job | What it runs |
| --- | --- |
| `rust` | `cargo test --features mcp,gateway`, gRPC lib tests, and a proto stub diff check |
| `superglue-js` | `npm ci`, native debug build, TypeScript facade, `node test/run.mjs` |
| `superglue-py` | `maturin develop` on CPython 3.12, then `pytest` |
| `superglue-kt` | `./gradlew :superglue-android:testDebugUnitTest` with `SUPERGLUE_FORCE_HOST_JNI=1` |

## Publish

The Rust crate is not published to crates.io. Depend on this git repository.

Intended registry names:

| Package | Registry |
| --- | --- |
| `superglue-py` | PyPI |
| `superglue-js` and the platform packages | npm |
| `com.superglue.kt:superglue-android` | Maven Central |

The workflow files `publish-pypi.yml`, `publish-npm.yml`, and `publish-maven.yml` are not in this repository.
A GitHub Release does not upload those packages until those workflows exist.
Tag a release as `v` plus the workspace version. Example: workspace `1.1.0` uses tag `v1.1.0`.

When you add the workflows, use this setup:

1. On PyPI, add a pending publisher for project `superglue-py`. Set the owner to `pinhole-io`, the repository to `superglue`, the workflow to `publish-pypi.yml`, and the environment to `pypi`.
2. Create GitHub environments named `pypi`, `npm`, and `maven`. Add required reviewers when a person must approve each upload.
3. Put an npm automation token in secret `NPM_TOKEN` on the `npm` environment. The token must be allowed to publish `superglue-js` and the platform names `superglue-js-linux-x64-gnu`, `superglue-js-linux-arm64-gnu`, `superglue-js-darwin-x64`, and `superglue-js-darwin-arm64`.
4. On [central.sonatype.com](https://central.sonatype.com), verify the namespace `com.superglue.kt`. Put the user token in `CENTRAL_TOKEN`. Put the ASCII-armored GPG private key in `GPG_PRIVATE_KEY`. Put the key passphrase in `GPG_PASSPHRASE`. Store those three secrets on the `maven` environment.

The workflows must read those secrets at publish time and must not print the values.
A person who can publish a release on this repository can publish these packages.
Protect the `pypi`, `npm`, and `maven` environments when that risk is too open.
