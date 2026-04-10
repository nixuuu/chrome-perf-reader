use std::fmt::Write as _;

use crate::analysis::{Detached, Diff, Summary, TopRetainers};
use crate::lighthouse;
use crate::report::{
    fmt_bytes, fmt_delta_bytes, fmt_duration_us, fmt_ms, fmt_num, fmt_signed, sanitize_name,
    truncate,
};
use crate::trace;

pub fn render_summary(s: &Summary) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Heap snapshot — {}", s.file_name);
    out.push('\n');
    let _ = writeln!(out, "- File: {}", fmt_bytes(s.file_size));
    let _ = writeln!(out, "- Nodes: {}", fmt_num(s.node_count as u64));
    let _ = writeln!(out, "- Edges: {}", fmt_num(s.edge_count as u64));
    let _ = writeln!(out, "- Strings: {}", fmt_num(s.string_count as u64));
    let _ = writeln!(out, "- Self size (sum): {}", fmt_bytes(s.total_self_size));
    let _ = writeln!(
        out,
        "- Retained from GC root: {}",
        fmt_bytes(s.total_retained_from_root)
    );
    if s.unreachable_count > 0 {
        let _ = writeln!(
            out,
            "- Unreachable: {} nodes / {}",
            fmt_num(s.unreachable_count as u64),
            fmt_bytes(s.unreachable_self_size)
        );
    }
    out.push('\n');

    out.push_str("## Nodes by type\n\n");
    out.push_str("| Type | Count | Self size | % self |\n");
    out.push_str("| --- | ---: | ---: | ---: |\n");
    let total = s.total_self_size.max(1);
    for b in &s.node_type_histogram {
        #[allow(clippy::cast_precision_loss)]
        let pct = (b.total_self_size as f64 / total as f64) * 100.0;
        let _ = writeln!(
            out,
            "| {} | {} | {} | {:.1}% |",
            b.name,
            fmt_num(b.count),
            fmt_bytes(b.total_self_size),
            pct
        );
    }
    out.push('\n');

    out.push_str("## Edges by type\n\n");
    out.push_str("| Type | Count |\n");
    out.push_str("| --- | ---: |\n");
    for b in &s.edge_type_histogram {
        let _ = writeln!(out, "| {} | {} |", b.name, fmt_num(b.count));
    }
    out.push('\n');

    out
}

pub fn render_top(t: &TopRetainers) -> String {
    let mut out = String::new();

    if !t.by_retained.is_empty() {
        let _ = writeln!(out, "## Top {} by retained size", t.by_retained.len());
        out.push('\n');
        out.push_str("| # | Type | Name | Self | Retained | Edges | ID |\n");
        out.push_str("| ---: | --- | --- | ---: | ---: | ---: | ---: |\n");
        for r in &t.by_retained {
            let _ = writeln!(
                out,
                "| {} | {} | `{}` | {} | {} | {} | {} |",
                r.rank,
                r.type_name,
                escape(&truncate(&sanitize_name(&r.name), 40)),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.edge_count,
                r.id
            );
        }
        out.push('\n');
    }

    if !t.by_self.is_empty() {
        let _ = writeln!(out, "## Top {} by self size", t.by_self.len());
        out.push('\n');
        out.push_str("| # | Type | Name | Self | Retained | Edges | ID |\n");
        out.push_str("| ---: | --- | --- | ---: | ---: | ---: | ---: |\n");
        for r in &t.by_self {
            let _ = writeln!(
                out,
                "| {} | {} | `{}` | {} | {} | {} | {} |",
                r.rank,
                r.type_name,
                escape(&truncate(&sanitize_name(&r.name), 40)),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.edge_count,
                r.id
            );
        }
        out.push('\n');
    }

    out
}

pub fn render_detached(d: &Detached) -> String {
    let mut out = String::new();
    out.push_str("## Detached candidates\n\n");
    if d.total_count == 0 {
        out.push_str("None detected (detachedness flag = 0 for all nodes, or field absent).\n\n");
        return out;
    }
    let _ = writeln!(
        out,
        "Total: {} detached nodes, {} retained.",
        fmt_num(d.total_count as u64),
        fmt_bytes(d.total_retained)
    );
    out.push('\n');

    let _ = writeln!(out, "### Top {}", d.top.len());
    out.push('\n');
    out.push_str("| # | Type | Name | Self | Retained | ID | Retained via (dominator chain) |\n");
    out.push_str("| ---: | --- | --- | ---: | ---: | ---: | --- |\n");
    for (i, r) in d.top.iter().enumerate() {
        let chain: Vec<String> = r
            .dominator_chain
            .iter()
            .map(|step| {
                format!(
                    "{}({})",
                    step.type_name,
                    escape(&truncate(&sanitize_name(&step.name), 18))
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "| {} | {} | `{}` | {} | {} | {} | {} |",
            i + 1,
            r.type_name,
            escape(&truncate(&sanitize_name(&r.name), 30)),
            fmt_bytes(r.self_size),
            fmt_bytes(r.retained_size),
            r.id,
            chain.join(" \u{2190} ")
        );
    }
    out.push('\n');

    out
}

#[allow(clippy::cast_possible_wrap)]
pub fn render_diff(d: &Diff) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Diff — {} \u{2192} {}", d.a_name, d.b_name);
    out.push('\n');

    let delta_nodes = d.b_nodes as i64 - d.a_nodes as i64;
    let delta_self = i128::from(d.b_self) - i128::from(d.a_self);
    let delta_ret = i128::from(d.b_retained) - i128::from(d.a_retained);
    let _ = writeln!(
        out,
        "- Nodes: {} \u{2192} {} ({})",
        fmt_num(d.a_nodes as u64),
        fmt_num(d.b_nodes as u64),
        fmt_signed(delta_nodes)
    );
    let _ = writeln!(
        out,
        "- Self size: {} \u{2192} {} ({})",
        fmt_bytes(d.a_self),
        fmt_bytes(d.b_self),
        fmt_delta_bytes(delta_self)
    );
    let _ = writeln!(
        out,
        "- Retained: {} \u{2192} {} ({})",
        fmt_bytes(d.a_retained),
        fmt_bytes(d.b_retained),
        fmt_delta_bytes(delta_ret)
    );
    out.push('\n');

    out.push_str("## \u{0394} by type\n\n");
    out.push_str("| Type | Count A | Count B | \u{0394} count | Size A | Size B | \u{0394} size |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for td in d.type_deltas.iter().take(20) {
        let dc = td.count_b.cast_signed() - td.count_a.cast_signed();
        let ds = i128::from(td.size_b) - i128::from(td.size_a);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            td.name,
            fmt_num(td.count_a),
            fmt_num(td.count_b),
            fmt_signed(dc),
            fmt_bytes(td.size_a),
            fmt_bytes(td.size_b),
            fmt_delta_bytes(ds)
        );
    }
    out.push('\n');

    if !d.new_nodes.is_empty() {
        let _ = writeln!(out, "## Top {} new nodes (in B, not A)", d.new_nodes.len());
        out.push('\n');
        out.push_str("| Type | Name | Self | Retained | ID |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
        for r in &d.new_nodes {
            let _ = writeln!(
                out,
                "| {} | `{}` | {} | {} | {} |",
                r.type_name,
                escape(&truncate(&sanitize_name(&r.name), 40)),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.id
            );
        }
        out.push('\n');
    }

    if !d.gone_nodes.is_empty() {
        let _ = writeln!(out, "## Top {} gone nodes (in A, not B)", d.gone_nodes.len());
        out.push('\n');
        out.push_str("| Type | Name | Self | Retained | ID |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
        for r in &d.gone_nodes {
            let _ = writeln!(
                out,
                "| {} | `{}` | {} | {} | {} |",
                r.type_name,
                escape(&truncate(&sanitize_name(&r.name), 40)),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.id
            );
        }
        out.push('\n');
    }

    out
}

#[allow(clippy::too_many_lines)]
pub fn render_lighthouse(r: &lighthouse::LighthouseReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Lighthouse — {}", r.file_name);
    out.push('\n');
    let _ = writeln!(out, "- URL: {}", r.url);
    if r.final_url != r.url && !r.final_url.is_empty() {
        let _ = writeln!(out, "- Final URL: {}", r.final_url);
    }
    let _ = writeln!(out, "- Fetched: {}", r.fetch_time);
    let _ = writeln!(out, "- Lighthouse: v{}", r.lighthouse_version);
    let _ = writeln!(out, "- Device: {} ({})", r.form_factor, r.throttling.method);
    let _ = writeln!(
        out,
        "- Throttling: RTT {}ms, {:.0} Kbps, {}x CPU slowdown",
        r.throttling.rtt_ms, r.throttling.throughput_kbps, r.throttling.cpu_slowdown
    );
    let _ = writeln!(out, "- File: {}", fmt_bytes(r.file_size));
    out.push('\n');

    if let Some(err) = &r.runtime_error {
        let _ = writeln!(out, "> **Runtime error**: {} — {}", err.code, err.message);
        out.push('\n');
    }
    for w in &r.run_warnings {
        let _ = writeln!(out, "> **Warning**: {w}");
    }
    if !r.run_warnings.is_empty() {
        out.push('\n');
    }

    // Category scores.
    if r.categories.iter().any(|c| c.score.is_some()) {
        out.push_str("## Scores\n\n");
        out.push_str("| Category | Score |\n");
        out.push_str("| --- | ---: |\n");
        for cat in &r.categories {
            let score = cat.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            let _ = writeln!(out, "| {} | {} |", cat.title, score);
        }
        out.push('\n');
    }

    // Metrics.
    let has_metrics = r.metrics.iter().any(|m| m.numeric_value.is_some());
    if has_metrics {
        out.push_str("## Core Web Vitals / metrics\n\n");
        out.push_str("| Metric | Value | Score |\n");
        out.push_str("| --- | ---: | ---: |\n");
        for m in &r.metrics {
            let val = if m.display_value.is_empty() {
                m.numeric_value.map_or_else(|| "n/a".to_owned(), |v| format_metric_value(v, &m.numeric_unit))
            } else {
                m.display_value.clone()
            };
            let score = m.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            let _ = writeln!(out, "| {} | {} | {} |", m.title, val, score);
        }
        out.push('\n');
    }

    // Diagnostics.
    for diag in &r.diagnostics {
        let title = if diag.display_value.is_empty() {
            diag.title.clone()
        } else {
            format!("{} ({})", diag.title, diag.display_value)
        };
        let _ = writeln!(out, "## {title}");
        out.push('\n');
        out.push_str("| Item | Value |\n");
        out.push_str("| --- | ---: |\n");
        for row in &diag.details {
            let _ = writeln!(out, "| {} | {} |", escape(&row.label), row.value);
        }
        out.push('\n');
    }

    // Opportunities.
    if !r.opportunities.is_empty() {
        out.push_str("## Opportunities\n\n");
        for opp in &r.opportunities {
            let savings = format_savings(opp.wasted_ms, opp.wasted_bytes);
            let _ = writeln!(out, "### {} {}", opp.title, savings);
            out.push('\n');
            if !opp.items.is_empty() {
                out.push_str("| URL | Wasted |\n");
                out.push_str("| --- | ---: |\n");
                for item in &opp.items {
                    let wasted = format_item_savings(item.wasted_ms, item.wasted_bytes);
                    let _ = writeln!(out, "| `{}` | {} |", escape(&item.url), wasted);
                }
                out.push('\n');
            }
        }
    }

    // Failed audits.
    if !r.failed_audits.is_empty() {
        let _ = writeln!(
            out,
            "## Failed audits ({} failed, {} passed)",
            r.failed_audits.len(),
            r.passed_audits
        );
        out.push('\n');
        out.push_str("| Category | Audit | Score |\n");
        out.push_str("| --- | --- | ---: |\n");
        for fa in &r.failed_audits {
            let score = fa.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            let _ = writeln!(out, "| {} | {} | {} |", fa.category, fa.title, score);
        }
        out.push('\n');
    }

    out
}

fn format_metric_value(v: f64, unit: &str) -> String {
    match unit {
        "millisecond" => fmt_ms(v),
        "unitless" => format!("{v:.3}"),
        _ => format!("{v:.1}"),
    }
}

fn format_savings(ms: Option<f64>, bytes: Option<f64>) -> String {
    let mut parts = Vec::new();
    if let Some(m) = ms
        && m > 0.0
    {
        parts.push(format!("save {}", fmt_ms(m)));
    }
    if let Some(b) = bytes
        && b > 0.0
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b_u64 = b as u64;
        parts.push(format!("save {}", fmt_bytes(b_u64)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("({})", parts.join(", "))
    }
}

fn format_item_savings(ms: Option<f64>, bytes: Option<f64>) -> String {
    let mut parts = Vec::new();
    if let Some(m) = ms
        && m > 0.0
    {
        parts.push(fmt_ms(m));
    }
    if let Some(b) = bytes
        && b > 0.0
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b_u64 = b as u64;
        parts.push(fmt_bytes(b_u64));
    }
    parts.join(", ")
}

fn escape(s: &str) -> String {
    s.replace('|', "\\|").replace('`', "'")
}

// ---------- trace renderers ----------

pub fn render_trace_summary(s: &trace::summary::TraceSummary) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Trace — {}", s.file_name);
    out.push('\n');
    let _ = writeln!(out, "- File: {}", fmt_bytes(s.file_size));
    let _ = writeln!(out, "- Duration: {}", fmt_duration_us(s.duration_us));
    let _ = writeln!(out, "- Events: {}", fmt_num(s.event_count as u64));
    let _ = writeln!(out, "- Processes: {}", s.processes.len());
    let _ = writeln!(
        out,
        "- Main thread: {} (pid {})",
        s.main_thread_name, s.main_thread_pid
    );
    out.push('\n');

    out.push_str("## Processes\n\n");
    out.push_str("| PID | Name | Threads | Events |\n");
    out.push_str("| ---: | --- | ---: | ---: |\n");
    for p in &s.processes {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            p.pid,
            p.name,
            p.thread_count,
            fmt_num(p.event_count as u64)
        );
    }
    out.push('\n');

    out.push_str("## Events by category\n\n");
    out.push_str("| Category | Count |\n");
    out.push_str("| --- | ---: |\n");
    for (cat, count) in s.category_histogram.iter().take(15) {
        let _ = writeln!(out, "| {} | {} |", truncate(cat, 60), fmt_num(*count as u64));
    }
    out.push('\n');

    let _ = writeln!(out, "## Long tasks (>50ms)");
    out.push('\n');
    if s.long_tasks.is_empty() {
        out.push_str("None.\n\n");
    } else {
        out.push_str("| # | Name | Duration | Offset | Thread |\n");
        out.push_str("| ---: | --- | ---: | ---: | --- |\n");
        for (i, lt) in s.long_tasks.iter().enumerate() {
            let _ = writeln!(
                out,
                "| {} | {} | {} | +{} | {} |",
                i + 1,
                lt.name,
                fmt_ms(lt.dur_ms),
                fmt_ms(lt.ts_offset_ms),
                lt.thread_name
            );
        }
        out.push('\n');
    }

    out
}

#[allow(clippy::cast_precision_loss)]
pub fn render_trace_frames(f: &trace::frames::FrameAnalysis) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Frame timing ({} frames over {})",
        fmt_num(f.frame_count as u64),
        fmt_ms(f.span_ms)
    );
    out.push('\n');
    if f.frame_count == 0 {
        out.push_str("No AnimationFrame events found.\n\n");
        return out;
    }
    let _ = writeln!(out, "- Avg FPS: {:.1}", f.avg_fps);
    let _ = writeln!(
        out,
        "- P50: {} | P95: {} | P99: {} | Max: {}",
        fmt_ms(f.p50_ms),
        fmt_ms(f.p95_ms),
        fmt_ms(f.p99_ms),
        fmt_ms(f.max_ms)
    );
    let total = f.frame_count.max(1);
    let _ = writeln!(
        out,
        "- Jank (>16.67ms): {} ({:.1}%)",
        f.jank_count,
        f.jank_count as f64 / total as f64 * 100.0
    );
    let _ = writeln!(out, "- Severe (>50ms): {}", f.severe_jank_count);
    out.push('\n');

    if !f.distribution.is_empty() {
        out.push_str("| Range | Count | % |\n");
        out.push_str("| --- | ---: | ---: |\n");
        for b in &f.distribution {
            let pct = b.count as f64 / total as f64 * 100.0;
            let _ = writeln!(
                out,
                "| {} | {} | {:.1}% |",
                b.label,
                fmt_num(b.count as u64),
                pct
            );
        }
        out.push('\n');
    }

    if !f.worst_frames.is_empty() {
        let _ = writeln!(out, "### Worst {} frames", f.worst_frames.len());
        out.push('\n');
        for fr in &f.worst_frames {
            let _ = writeln!(
                out,
                "**#{}** {} at +{}",
                fr.rank,
                fmt_ms(fr.dur_ms),
                fmt_ms(fr.ts_offset_ms)
            );
            if fr.breakdown.is_empty() {
                out.push_str("  (no matching timeline events)\n\n");
            } else {
                out.push('\n');
                out.push_str("| Event | Duration | Source |\n");
                out.push_str("| --- | ---: | --- |\n");
                for ev in &fr.breakdown {
                    let source = format_source(&ev.function_name, &ev.url);
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} |",
                        ev.name,
                        fmt_ms(ev.dur_ms),
                        source
                    );
                }
                out.push('\n');
            }
        }
    }

    out
}

fn format_source(func: &str, url: &str) -> String {
    match (func.is_empty(), url.is_empty()) {
        (true, true) => String::new(),
        (true, false) => url.to_owned(),
        (false, true) => format!("`{func}`"),
        (false, false) => format!("`{func}` ({url})"),
    }
}

pub fn render_trace_gc(g: &trace::gc::GcAnalysis) -> String {
    let mut out = String::new();
    out.push_str("## GC pressure\n\n");
    let _ = writeln!(
        out,
        "- Total GC: {} ({:.1}% of trace)",
        fmt_ms(g.total_gc_time_ms),
        g.gc_pct_of_trace
    );
    out.push('\n');

    out.push_str("| Type | Count | Total | Avg | Max |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    let buckets: &[(&str, &trace::gc::GcBucket)] = &[
        ("MajorGC", &g.major_gc),
        ("MinorGC", &g.minor_gc),
        ("Incremental marking", &g.incremental),
    ];
    for &(name, b) in buckets {
        if b.count > 0 {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                name,
                b.count,
                fmt_ms(b.total_ms),
                fmt_ms(b.avg_ms),
                fmt_ms(b.max_ms)
            );
        }
    }
    out.push('\n');

    if !g.top_events.is_empty() {
        let _ = writeln!(out, "### Top {} GC events", g.top_events.len());
        out.push('\n');
        out.push_str("| # | Type | Duration | Offset |\n");
        out.push_str("| ---: | --- | ---: | ---: |\n");
        for (i, ev) in g.top_events.iter().enumerate() {
            let _ = writeln!(
                out,
                "| {} | {} | {} | +{} |",
                i + 1,
                ev.name,
                fmt_ms(ev.dur_ms),
                fmt_ms(ev.ts_offset_ms)
            );
        }
        out.push('\n');
    }

    out
}

pub fn render_trace_hotspots(h: &trace::hotspots::HotspotAnalysis) -> String {
    let mut out = String::new();

    if !h.by_url.is_empty() {
        let _ = writeln!(out, "## JS execution by URL (top {})", h.by_url.len());
        out.push('\n');
        out.push_str("| URL | Calls | Total | Avg | Max |\n");
        out.push_str("| --- | ---: | ---: | ---: | ---: |\n");
        for u in &h.by_url {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | {} |",
                escape(&u.url),
                u.call_count,
                fmt_ms(u.total_ms),
                fmt_ms(u.avg_ms),
                fmt_ms(u.max_ms)
            );
        }
        out.push('\n');
    }

    if !h.by_function.is_empty() {
        let _ = writeln!(
            out,
            "## Top {} functions by total time",
            h.by_function.len()
        );
        out.push('\n');
        out.push_str("| Function | URL:line | Calls | Total | Max |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
        for f in &h.by_function {
            let loc = if f.line > 0 {
                format!("{}:{}", f.url, f.line)
            } else {
                f.url.clone()
            };
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | {} | {} |",
                escape(&f.function_name),
                escape(&loc),
                f.call_count,
                fmt_ms(f.total_ms),
                fmt_ms(f.max_ms)
            );
        }
        out.push('\n');
    }

    if h.by_url.is_empty() && h.by_function.is_empty() {
        out.push_str("## JS execution hotspots\n\nNo FunctionCall/EvaluateScript events found.\n\n");
    }

    out
}
