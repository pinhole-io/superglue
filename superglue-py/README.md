# superglue-py

Python bindings for the [superglue](https://github.com/pinhole-io/superglue) Rust core.

The import name is `superglue`. The package name is `superglue-py`.

## Install

```bash
pip install superglue-py
```

Wheels cover CPython 3.12, 3.13, and 3.14, including free-threaded 3.14. Platforms: Linux x86_64, Linux aarch64, macOS x86_64, and macOS Apple Silicon.

On another platform, install the source distribution from PyPI. You need a Rust toolchain. You can also download a wheel from the GitHub release and pass that file to `pip install`.

## Build from this repository

```bash
uv venv --python 3.14t
./dev.sh
```

`requires-python` is `>=3.12`.
