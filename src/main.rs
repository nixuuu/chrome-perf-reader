mod analysis;
mod cli;
mod lighthouse;
mod parser;
mod report;
mod trace;

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::Parser as _;

use cli::{Cli, Command, Format, SortBy};
use parser::HeapGraph;
use trace::TraceFile;

fn main() -> Result<()> {
    let args = Cli::parse();

    match (args.cmd, args.file) {
        // Heap subcommands.
        (Some(Command::Summary { file }), _) => cmd_summary(&file, args.format),
        (Some(Command::Top { file, by }), _) => cmd_top(&file, by, args.format, args.limit),
        (Some(Command::Detached { file }), _) => cmd_detached(&file, args.format, args.limit),
        (Some(Command::Diff { a, b }), _) => cmd_diff(&a, &b, args.format, args.limit),
        // Trace subcommands.
        (Some(Command::Trace { file }), _) => {
            cmd_trace_full(&file, args.format, args.limit, args.threshold)
        }
        (Some(Command::Frames { file }), _) => cmd_trace_frames(&file, args.format, args.limit),
        (Some(Command::Gc { file }), _) => cmd_trace_gc(&file, args.format, args.limit),
        (Some(Command::Hotspots { file }), _) => {
            cmd_trace_hotspots(&file, args.format, args.limit)
        }
        // Lighthouse subcommand.
        (Some(Command::Lighthouse { file }), _) => cmd_lighthouse(&file, args.format),
        // Auto-detect.
        (None, Some(file)) => cmd_auto(&file, args.format, args.limit, args.threshold),
        (None, None) => {
            eprintln!("usage: chrome-perf-reader [--format markdown|text] <FILE>");
            eprintln!("       chrome-perf-reader diff <A> <B>");
            eprintln!("       chrome-perf-reader <summary|top|detached> <FILE>  (heap)");
            eprintln!("       chrome-perf-reader <trace|frames|gc|hotspots> <FILE>  (trace)");
            eprintln!("       chrome-perf-reader lighthouse <FILE>  (lighthouse)");
            std::process::exit(2);
        }
    }
}

// ---------- auto-detect ----------

fn cmd_auto(file: &Path, format: Format, limit: usize, threshold: f64) -> Result<()> {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if ext == "heapsnapshot" {
        return cmd_heap_full(file, format, limit);
    }

    // For .json / .json.gz / .gz, peek content.
    if ext == "json" || ext == "gz" || file.to_string_lossy().ends_with(".json.gz") {
        if lighthouse::looks_like_lighthouse(file) {
            return cmd_lighthouse(file, format);
        }
        if trace::parser::looks_like_trace(file) {
            return cmd_trace_full(file, format, limit, threshold);
        }
        // Try heapsnapshot (some people save them as .json).
        return cmd_heap_full(file, format, limit);
    }

    // Unknown extension: try heapsnapshot, then trace, then lighthouse.
    if cmd_heap_full(file, format, limit).is_ok() {
        return Ok(());
    }
    if lighthouse::looks_like_lighthouse(file) {
        return cmd_lighthouse(file, format);
    }
    if trace::parser::looks_like_trace(file) {
        return cmd_trace_full(file, format, limit, threshold);
    }

    Err(anyhow!(
        "Could not detect file format for {}. Use a subcommand to specify.",
        file.display()
    ))
}

// ---------- heap commands ----------

struct HeapLoaded {
    graph: HeapGraph,
    name: String,
    size: u64,
}

fn load_heap(path: &Path) -> Result<HeapLoaded> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_owned();
    let size = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    let graph = HeapGraph::load(path)?;
    Ok(HeapLoaded { graph, name, size })
}

fn cmd_heap_full(file: &Path, format: Format, limit: usize) -> Result<()> {
    let loaded = load_heap(file)?;
    let dom = analysis::dominator::compute(&loaded.graph);
    let summary =
        analysis::summary::compute(&loaded.graph, &dom, loaded.name.clone(), loaded.size);
    let top = analysis::retainers::compute(&loaded.graph, &dom, limit);
    let det = analysis::detached::compute(&loaded.graph, &dom, limit);

    match format {
        Format::Markdown => {
            print!("{}", report::markdown::render_summary(&summary));
            print!("{}", report::markdown::render_top(&top));
            print!("{}", report::markdown::render_detached(&det));
        }
        Format::Text => {
            print!("{}", report::text::render_summary(&summary));
            print!("{}", report::text::render_top(&top));
            print!("{}", report::text::render_detached(&det));
        }
    }
    Ok(())
}

fn cmd_summary(file: &Path, format: Format) -> Result<()> {
    let loaded = load_heap(file)?;
    let dom = analysis::dominator::compute(&loaded.graph);
    let summary = analysis::summary::compute(&loaded.graph, &dom, loaded.name, loaded.size);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_summary(&summary)),
        Format::Text => print!("{}", report::text::render_summary(&summary)),
    }
    Ok(())
}

fn cmd_top(file: &Path, by: SortBy, format: Format, limit: usize) -> Result<()> {
    let loaded = load_heap(file)?;
    let dom = analysis::dominator::compute(&loaded.graph);
    let mut top = analysis::retainers::compute(&loaded.graph, &dom, limit);

    match by {
        SortBy::SelfSize => top.by_retained.clear(),
        SortBy::Retained => top.by_self.clear(),
        SortBy::Both => {}
    }

    match format {
        Format::Markdown => print!("{}", report::markdown::render_top(&top)),
        Format::Text => print!("{}", report::text::render_top(&top)),
    }
    Ok(())
}

fn cmd_detached(file: &Path, format: Format, limit: usize) -> Result<()> {
    let loaded = load_heap(file)?;
    let dom = analysis::dominator::compute(&loaded.graph);
    let det = analysis::detached::compute(&loaded.graph, &dom, limit);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_detached(&det)),
        Format::Text => print!("{}", report::text::render_detached(&det)),
    }
    Ok(())
}

fn cmd_diff(a: &Path, b: &Path, format: Format, limit: usize) -> Result<()> {
    let la = load_heap(a)?;
    let lb = load_heap(b)?;
    let da = analysis::dominator::compute(&la.graph);
    let db = analysis::dominator::compute(&lb.graph);
    let diff =
        analysis::diff::compute(&la.graph, &da, la.name, &lb.graph, &db, lb.name, limit);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_diff(&diff)),
        Format::Text => print!("{}", report::text::render_diff(&diff)),
    }
    Ok(())
}

// ---------- trace commands ----------

struct TraceLoaded {
    trace: TraceFile,
    name: String,
    size: u64,
}

fn load_trace(path: &Path) -> Result<TraceLoaded> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_owned();
    let size = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    let trace = TraceFile::load(path)?;
    Ok(TraceLoaded { trace, name, size })
}

fn cmd_trace_full(file: &Path, format: Format, limit: usize, threshold: f64) -> Result<()> {
    let loaded = load_trace(file)?;
    let summary =
        trace::summary::compute(&loaded.trace, loaded.name, loaded.size, threshold);
    let frames = trace::frames::compute(&loaded.trace, limit);
    let gc = trace::gc::compute(&loaded.trace, limit);
    let hotspots = trace::hotspots::compute(&loaded.trace, limit);

    match format {
        Format::Markdown => {
            print!("{}", report::markdown::render_trace_summary(&summary));
            print!("{}", report::markdown::render_trace_frames(&frames));
            print!("{}", report::markdown::render_trace_gc(&gc));
            print!("{}", report::markdown::render_trace_hotspots(&hotspots));
        }
        Format::Text => {
            print!("{}", report::text::render_trace_summary(&summary));
            print!("{}", report::text::render_trace_frames(&frames));
            print!("{}", report::text::render_trace_gc(&gc));
            print!("{}", report::text::render_trace_hotspots(&hotspots));
        }
    }
    Ok(())
}

fn cmd_trace_frames(file: &Path, format: Format, limit: usize) -> Result<()> {
    let loaded = load_trace(file)?;
    let frames = trace::frames::compute(&loaded.trace, limit);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_trace_frames(&frames)),
        Format::Text => print!("{}", report::text::render_trace_frames(&frames)),
    }
    Ok(())
}

fn cmd_trace_gc(file: &Path, format: Format, limit: usize) -> Result<()> {
    let loaded = load_trace(file)?;
    let gc = trace::gc::compute(&loaded.trace, limit);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_trace_gc(&gc)),
        Format::Text => print!("{}", report::text::render_trace_gc(&gc)),
    }
    Ok(())
}

fn cmd_trace_hotspots(file: &Path, format: Format, limit: usize) -> Result<()> {
    let loaded = load_trace(file)?;
    let hotspots = trace::hotspots::compute(&loaded.trace, limit);
    match format {
        Format::Markdown => print!("{}", report::markdown::render_trace_hotspots(&hotspots)),
        Format::Text => print!("{}", report::text::render_trace_hotspots(&hotspots)),
    }
    Ok(())
}

// ---------- lighthouse commands ----------

fn cmd_lighthouse(file: &Path, format: Format) -> Result<()> {
    let report = lighthouse::LighthouseReport::load(file)?;
    match format {
        Format::Markdown => print!("{}", report::markdown::render_lighthouse(&report)),
        Format::Text => print!("{}", report::text::render_lighthouse(&report)),
    }
    Ok(())
}
