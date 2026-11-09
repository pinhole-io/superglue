# superglue-js examples

Scripts use the same numbers as the Python and Rust examples.
The parity table is in [`../../examples/README.md`](../../examples/README.md).

## Run

From `superglue-js/`:

```bash
npm install
npm run build
export OPENAI_API_KEY=sk-...
node examples/01_simple_completion.mjs
```

`08_custom_base_url.mjs` reads `OPENAI_BASE_URL`.
`28_mcp_tools.mjs` runs only when `MCP_RUN=1`.
`34_context_optimization_live_benchmark.mjs` compares two context configs on one prompt.
`35_code_tool_mode.mjs` needs a native build that includes the `code` feature. The binding crate enables that feature.
`37_systemone.mjs` needs `TYPESAFE_API_KEY`. It exits 0 when that variable is unset.
`38_file_transcription.mjs` needs `XAI_API_KEY` by default. It exits 0 when that key is unset.
`40_judge.mjs` and `42_judge_advanced.mjs` need `TYPESAFE_API_KEY`. They exit 0 when that key is unset.
`41_structured_output.mjs` needs `OPENAI_API_KEY`. It exits 0 when that key is unset.
`43_graph.mjs` needs `OPENAI_API_KEY`. It drafts, then polishes. `TYPESAFE_API_KEY` adds a judge.
`44_multi_judge.mjs` needs `OPENAI_API_KEY` and `TYPESAFE_API_KEY`. Three judges score one draft. It exits 0 when either key is unset.

## Example 21

`SimpleExecutor`, `AgentExecutor`, and `AgentStructuredExecutor` exist only in Python.
`21_executors.mjs` prints that fact.
The Python script is [`../../superglue-py/examples/21_executors.py`](../../superglue-py/examples/21_executors.py).

Rust equivalents run from the crate root:

```bash
cargo run --example 01_simple_completion
```
