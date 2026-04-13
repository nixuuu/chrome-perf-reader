# chrome-perf-reader

CLI that parses Chrome DevTools performance artifacts and prints human- and LLM-friendly reports.

Supports three file formats:

- **Heap snapshots** (`.heapsnapshot`) — memory analysis with dominator tree, retained sizes, detached DOM detection, and snapshot diffs
- **Performance traces** (`.json` / `.json.gz`) — frame timing, jank detection, GC pressure, JS execution hotspots
- **Lighthouse reports** (`.json`) — Core Web Vitals, category scores, diagnostics, optimization opportunities

Output is designed to be pasted directly into an LLM for analysis, or read by a human in a terminal.

## Installation

### Shell (Linux / macOS)

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/nixuuu/chrome-perf-reader/releases/latest/download/chrome-perf-reader-installer.sh | sh
```

### PowerShell (Windows)

```powershell
powershell -c "irm https://github.com/nixuuu/chrome-perf-reader/releases/latest/download/chrome-perf-reader-installer.ps1 | iex"
```

### Homebrew

```sh
brew tap nixuuu/tap
brew install chrome-perf-reader
```

### From source

```sh
cargo install --git https://github.com/nixuuu/chrome-perf-reader
```

## Usage

```sh
# Auto-detect file format from a single file.
chrome-perf-reader path/to/file

# Explicit subcommands.
chrome-perf-reader summary    snapshot.heapsnapshot
chrome-perf-reader top        snapshot.heapsnapshot --by retained
chrome-perf-reader detached   snapshot.heapsnapshot
chrome-perf-reader diff       before.heapsnapshot after.heapsnapshot

chrome-perf-reader trace      trace.json.gz
chrome-perf-reader frames     trace.json.gz
chrome-perf-reader gc         trace.json.gz
chrome-perf-reader hotspots   trace.json.gz

chrome-perf-reader lighthouse report.json
```

### Global options

| Flag | Default | Description |
| --- | --- | --- |
| `--format` | `markdown` | Output format: `markdown` or `text` |
| `--limit` | `20` | Rows to show in top / diff / hotspot tables |
| `--threshold` | `50.0` | Long task threshold in ms (trace only) |

## What you get

### Heap snapshots

- Summary with node/edge counts, self size, retained size from GC root
- Node and edge type histograms
- Top-N retainers by self size and by retained size (with dominator-tree retained sizes)
- Detached DOM candidates with dominator chain
- Full snapshot diff: added/removed nodes, per-type delta

### Traces

- Process and thread inventory
- Long tasks (`>50ms` by default) across all threads
- Frame timing: P50/P95/P99, jank percentage, FPS, distribution buckets
- Per-frame breakdown showing what ran inside each slow frame (`FunctionCall`, `Layout`, `Paint`, etc.) with source URLs and function names
- GC pressure: Major, Minor, and incremental marking pauses with totals
- JS execution hotspots aggregated by URL and by function

### Lighthouse

- Category scores (Performance, Accessibility, Best Practices, SEO)
- Core Web Vitals (FCP, LCP, TBT, CLS, TTI, Speed Index, Max FID)
- Main-thread work breakdown, JS execution time per script, resource summary, network payloads
- Optimization opportunities with estimated savings (unused JS/CSS, render-blocking, minification, compression)
- Failed audits grouped by category
- Handles errored runs (e.g. `NO_FCP`) gracefully

## Example

```sh
$ chrome-perf-reader trace.json.gz

# Trace — trace.json.gz

- File: 4.77 MB
- Duration: 12.88s
- Events: 75,503
- Processes: 4
- Main thread: CrRendererMain (pid 1258)

## Frame timing (1,588 frames over 10.27s)

- Avg FPS: 154.7
- P50: 4.2ms | P95: 12.1ms | P99: 18.3ms | Max: 23.4ms
- Jank (>16.67ms): 42 frames (2.6%)

### Worst 20 frames

**#1** 234.56ms at +3.42s

| Event | Duration | Source |
| --- | ---: | --- |
| FunctionCall | 180.23ms | `renderChart` (https://example.com/app.js) |
| Layout | 42.10ms |  |
...
```

## Why

Chrome DevTools has excellent visual tools for these artifacts, but:

- Screenshots don't work well in LLM conversations
- Export formats are not human-readable at the terminal
- Diffing two heap snapshots in the UI is tedious
- You can't grep the UI

This tool produces output that is copyable, diffable, and understandable by both humans and language models.

## File format notes

- Heap snapshots can be saved as `.heapsnapshot` from the Memory tab
- Traces can be saved as `.json` or `.json.gz` from the Performance tab (gzip is auto-detected)
- Lighthouse JSON reports come from the Lighthouse tab or the CLI (`lighthouse https://… --output json`)

## License

MIT
