# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`chrome-perf-reader` is a Rust CLI that parses three Chrome DevTools artifact formats — V8 heap snapshots (`.heapsnapshot`), Chrome trace events (`.json` / `.json.gz`), and Lighthouse JSON reports — and prints human- and LLM-friendly summaries. Output is meant to be pasted into an LLM conversation or read in a terminal, so formatting is deliberately minimal and deterministic.

Rust edition is **2024**. Lints are strict: `warnings`, `clippy::all`, `clippy::pedantic`, and `clippy::nursery` are all `deny`. Any new code must pass `cargo clippy` with zero warnings or the build fails.

## Commands

```sh
cargo build                       # debug build
cargo build --release             # release build (LTO + single codegen unit)
cargo run -- <FILE>               # run against a file (auto-detect)
cargo run -- trace tests/x.json   # run a specific subcommand
cargo clippy --all-targets        # must be clean — lints are deny
cargo fmt                         # format
cargo test                        # no tests currently exist, but the target works
```

There are no integration tests yet. When adding them, keep fixtures small — real heap snapshots and traces are large and would bloat the repo.

## Architecture

The pipeline has three stages for each artifact type: **parse → analyze → render**. The renderer never touches raw data; analysis modules compute typed structs (e.g. `Summary`, `TopRetainers`, `Detached`, `Diff`, trace `Frames`/`Gc`/`Hotspots`, `LighthouseReport`) which `report::markdown` and `report::text` both consume. This means a new output format only needs a new renderer, not changes to analysis.

`src/main.rs` is the dispatcher: it parses CLI args (`src/cli.rs`, clap derive), loads the input, picks an analysis function, and calls one of the two renderers. `cmd_auto` does format detection by extension then by content-sniffing (`lighthouse::looks_like_lighthouse`, `trace::parser::looks_like_trace`), falling back to heap snapshot.

### Heap snapshot pipeline (`src/parser` + `src/analysis`)

- `parser::raw` deserializes the V8 heap snapshot JSON (a schema with `nodes`/`edges` as flat integer arrays and a separate string table).
- `parser::graph::HeapGraph` turns that raw shape into an indexed graph: nodes and edges stay as flat `Vec<u64>`/`Vec<u32>` (no per-node struct), edge `to_node` byte-offsets are converted to real node indices in place, and a CSR prefix sum (`first_edge`) lets you iterate node `i`'s edges as `edges[first_edge[i]..first_edge[i+1]]`. Field offsets (`nf_type`, `nf_self_size`, `ef_to_node`, …) come from the snapshot's `meta` and are stored on the graph — always go through them, never hardcode indices.
- `analysis::dominator` computes the dominator tree from the GC root; every other heap analysis depends on its output for retained sizes. Compute it once per snapshot and pass it in.
- `analysis::{summary, retainers, detached, diff}` each take `&HeapGraph` + `&Dominators` and return a plain struct. `diff` loads two graphs and matches nodes across them.

### Trace pipeline (`src/trace`)

- `trace::parser::TraceFile::load` handles both raw `.json` and gzipped `.json.gz` (gzip is detected by magic bytes, not extension), and parses either the object form (`{"traceEvents": [...]}`) or bare array form.
- Metadata events populate `thread_names`, `process_names`, and `main_thread` detection. `duration_us` is `max_ts - min_ts` across events with `ts > 0`.
- `trace::{summary, frames, gc, hotspots}` each consume `&TraceFile` and produce typed structs consumed by the renderers. The `threshold` CLI flag (long-task ms) is plumbed through `summary`.

### Lighthouse pipeline (`src/lighthouse`)

Single `parser.rs` that deserializes the Lighthouse JSON report into a struct graph of categories, audits, and numeric values. `looks_like_lighthouse` is a cheap content sniff for auto-detect. Errored runs (e.g. `NO_FCP`) are handled without panicking — the renderer shows them as N/A rather than aborting.

### Rendering (`src/report`)

`report::markdown` and `report::text` export a parallel set of `render_*` functions — one per analysis struct — and both import shared helpers from `report/mod.rs` (`fmt_bytes`, `fmt_num`, `fmt_ms`, `fmt_duration_us`, `truncate`, `sanitize_name`, `fmt_signed`, `fmt_delta_bytes`). When adding a new analysis, add both renderers in the same commit so text and markdown stay in lockstep. String truncation must go through `truncate` (it's char-aware, unlike byte slicing).

## Conventions

- Errors flow through `anyhow::Result` with `.with_context(|| …)` at I/O boundaries. Don't swap this out for custom error types unless you're prepared to migrate all call sites.
- Numeric casts that trip clippy pedantic/nursery are sometimes unavoidable (e.g. in `fmt_signed`, `fmt_delta_bytes`). Use narrowly scoped `#[allow(clippy::…)]` with a comment, the way the existing code does — don't relax lints project-wide.
- The heap graph intentionally avoids per-node structs for memory reasons (snapshots can have tens of millions of nodes). Keep analyses iterating over the flat arrays via the CSR index; don't materialize node structs.
- CLI flags `--format`, `--limit`, `--threshold` are global (`global = true` on clap). New output knobs should follow the same pattern so they work with both auto-detect and subcommands.
