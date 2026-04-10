//! Plain-text renderer. Uses aligned ASCII tables.

use std::fmt::Write as _;

use crate::analysis::{Detached, Diff, Summary, TopRetainers};
use crate::lighthouse;
use crate::report::{
    fmt_bytes, fmt_delta_bytes, fmt_duration_us, fmt_ms, fmt_num, fmt_signed, sanitize_name,
    truncate,
};
use crate::trace;

enum Align {
    Left,
    Right,
}

struct Table {
    headers: Vec<String>,
    aligns: Vec<Align>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new(headers: &[&str], aligns: Vec<Align>) -> Self {
        assert_eq!(headers.len(), aligns.len());
        Self {
            headers: headers.iter().map(|s| (*s).to_owned()).collect(),
            aligns,
            rows: Vec::new(),
        }
    }

    fn row(&mut self, cells: Vec<String>) {
        assert_eq!(cells.len(), self.headers.len());
        self.rows.push(cells);
    }

    fn render(&self, out: &mut String) {
        let cols = self.headers.len();
        let mut widths = vec![0usize; cols];
        for (i, h) in self.headers.iter().enumerate() {
            widths[i] = widths[i].max(display_width(h));
        }
        for r in &self.rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(display_width(c));
            }
        }

        // Header
        for (i, width) in widths.iter().enumerate().take(cols) {
            if i > 0 {
                out.push_str("  ");
            }
            push_padded(out, &self.headers[i], *width, &self.aligns[i]);
        }
        out.push('\n');

        // Separator
        for (i, width) in widths.iter().enumerate().take(cols) {
            if i > 0 {
                out.push_str("  ");
            }
            for _ in 0..*width {
                out.push('-');
            }
        }
        out.push('\n');

        // Rows
        for r in &self.rows {
            for i in 0..cols {
                if i > 0 {
                    out.push_str("  ");
                }
                push_padded(out, &r[i], widths[i], &self.aligns[i]);
            }
            out.push('\n');
        }
    }
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn push_padded(out: &mut String, s: &str, width: usize, align: &Align) {
    let w = display_width(s);
    let pad = width.saturating_sub(w);
    match align {
        Align::Left => {
            out.push_str(s);
            for _ in 0..pad {
                out.push(' ');
            }
        }
        Align::Right => {
            for _ in 0..pad {
                out.push(' ');
            }
            out.push_str(s);
        }
    }
}

fn heading(out: &mut String, level: usize, title: &str) {
    out.push('\n');
    out.push_str(title);
    out.push('\n');
    let ch = if level <= 1 { '=' } else { '-' };
    for _ in 0..title.chars().count() {
        out.push(ch);
    }
    out.push('\n');
    out.push('\n');
}

pub fn render_summary(s: &Summary) -> String {
    let mut out = String::new();
    heading(&mut out, 1, &format!("Heap snapshot — {}", s.file_name));

    let _ = writeln!(out, "File:                   {}", fmt_bytes(s.file_size));
    let _ = writeln!(out, "Nodes:                  {}", fmt_num(s.node_count as u64));
    let _ = writeln!(out, "Edges:                  {}", fmt_num(s.edge_count as u64));
    let _ = writeln!(out, "Strings:                {}", fmt_num(s.string_count as u64));
    let _ = writeln!(out, "Self size (sum):        {}", fmt_bytes(s.total_self_size));
    let _ = writeln!(out, "Retained from GC root:  {}", fmt_bytes(s.total_retained_from_root));
    if s.unreachable_count > 0 {
        let _ = writeln!(
            out,
            "Unreachable:            {} nodes / {}",
            fmt_num(s.unreachable_count as u64),
            fmt_bytes(s.unreachable_self_size)
        );
    }

    heading(&mut out, 2, "Nodes by type");
    let mut t = Table::new(
        &["Type", "Count", "Self size", "% self"],
        vec![Align::Left, Align::Right, Align::Right, Align::Right],
    );
    let total = s.total_self_size.max(1);
    for b in &s.node_type_histogram {
        #[allow(clippy::cast_precision_loss)]
        let pct = (b.total_self_size as f64 / total as f64) * 100.0;
        t.row(vec![
            b.name.clone(),
            fmt_num(b.count),
            fmt_bytes(b.total_self_size),
            format!("{pct:.1}%"),
        ]);
    }
    t.render(&mut out);

    heading(&mut out, 2, "Edges by type");
    let mut t = Table::new(
        &["Type", "Count"],
        vec![Align::Left, Align::Right],
    );
    for b in &s.edge_type_histogram {
        t.row(vec![b.name.clone(), fmt_num(b.count)]);
    }
    t.render(&mut out);

    out
}

pub fn render_top(tr: &TopRetainers) -> String {
    let mut out = String::new();

    if !tr.by_retained.is_empty() {
        heading(&mut out, 2, &format!("Top {} by retained size", tr.by_retained.len()));
        let mut t = Table::new(
            &["#", "Type", "Name", "Self", "Retained", "Edges", "ID"],
            vec![
                Align::Right,
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
            ],
        );
        for r in &tr.by_retained {
            t.row(vec![
                r.rank.to_string(),
                r.type_name.clone(),
                truncate(&sanitize_name(&r.name), 40),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.edge_count.to_string(),
                r.id.to_string(),
            ]);
        }
        t.render(&mut out);
    }

    if !tr.by_self.is_empty() {
        heading(&mut out, 2, &format!("Top {} by self size", tr.by_self.len()));
        let mut t = Table::new(
            &["#", "Type", "Name", "Self", "Retained", "Edges", "ID"],
            vec![
                Align::Right,
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
            ],
        );
        for r in &tr.by_self {
            t.row(vec![
                r.rank.to_string(),
                r.type_name.clone(),
                truncate(&sanitize_name(&r.name), 40),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.edge_count.to_string(),
                r.id.to_string(),
            ]);
        }
        t.render(&mut out);
    }

    out
}

pub fn render_detached(d: &Detached) -> String {
    let mut out = String::new();
    heading(&mut out, 2, "Detached candidates");
    if d.total_count == 0 {
        out.push_str("None detected.\n");
        return out;
    }
    let _ = writeln!(
        out,
        "Total: {} detached nodes, {} retained.",
        fmt_num(d.total_count as u64),
        fmt_bytes(d.total_retained)
    );
    out.push('\n');

    let mut t = Table::new(
        &["#", "Type", "Name", "Self", "Retained", "ID", "Retained via (dom chain)"],
        vec![
            Align::Right,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
    );
    for (i, r) in d.top.iter().enumerate() {
        let chain: Vec<String> = r
            .dominator_chain
            .iter()
            .map(|s| format!("{}({})", s.type_name, truncate(&sanitize_name(&s.name), 18)))
            .collect();
        t.row(vec![
            (i + 1).to_string(),
            r.type_name.clone(),
            truncate(&sanitize_name(&r.name), 30),
            fmt_bytes(r.self_size),
            fmt_bytes(r.retained_size),
            r.id.to_string(),
            chain.join(" <- "),
        ]);
    }
    t.render(&mut out);

    out
}

#[allow(clippy::cast_possible_wrap)]
pub fn render_diff(d: &Diff) -> String {
    let mut out = String::new();
    heading(&mut out, 1, &format!("Diff — {} -> {}", d.a_name, d.b_name));

    let delta_nodes = d.b_nodes as i64 - d.a_nodes as i64;
    let delta_self = i128::from(d.b_self) - i128::from(d.a_self);
    let delta_ret = i128::from(d.b_retained) - i128::from(d.a_retained);
    let _ = writeln!(
        out,
        "Nodes:      {} -> {} ({})",
        fmt_num(d.a_nodes as u64),
        fmt_num(d.b_nodes as u64),
        fmt_signed(delta_nodes)
    );
    let _ = writeln!(
        out,
        "Self size:  {} -> {} ({})",
        fmt_bytes(d.a_self),
        fmt_bytes(d.b_self),
        fmt_delta_bytes(delta_self)
    );
    let _ = writeln!(
        out,
        "Retained:   {} -> {} ({})",
        fmt_bytes(d.a_retained),
        fmt_bytes(d.b_retained),
        fmt_delta_bytes(delta_ret)
    );

    heading(&mut out, 2, "\u{0394} by type");
    let mut t = Table::new(
        &["Type", "Count A", "Count B", "\u{0394} count", "Size A", "Size B", "\u{0394} size"],
        vec![
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
    );
    for td in d.type_deltas.iter().take(20) {
        let dc = td.count_b.cast_signed() - td.count_a.cast_signed();
        let ds = i128::from(td.size_b) - i128::from(td.size_a);
        t.row(vec![
            td.name.clone(),
            fmt_num(td.count_a),
            fmt_num(td.count_b),
            fmt_signed(dc),
            fmt_bytes(td.size_a),
            fmt_bytes(td.size_b),
            fmt_delta_bytes(ds),
        ]);
    }
    t.render(&mut out);

    if !d.new_nodes.is_empty() {
        heading(&mut out, 2, &format!("Top {} new nodes (in B, not A)", d.new_nodes.len()));
        let mut t = Table::new(
            &["Type", "Name", "Self", "Retained", "ID"],
            vec![Align::Left, Align::Left, Align::Right, Align::Right, Align::Right],
        );
        for r in &d.new_nodes {
            t.row(vec![
                r.type_name.clone(),
                truncate(&sanitize_name(&r.name), 40),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.id.to_string(),
            ]);
        }
        t.render(&mut out);
    }

    if !d.gone_nodes.is_empty() {
        heading(&mut out, 2, &format!("Top {} gone nodes (in A, not B)", d.gone_nodes.len()));
        let mut t = Table::new(
            &["Type", "Name", "Self", "Retained", "ID"],
            vec![Align::Left, Align::Left, Align::Right, Align::Right, Align::Right],
        );
        for r in &d.gone_nodes {
            t.row(vec![
                r.type_name.clone(),
                truncate(&sanitize_name(&r.name), 40),
                fmt_bytes(r.self_size),
                fmt_bytes(r.retained_size),
                r.id.to_string(),
            ]);
        }
        t.render(&mut out);
    }

    out
}

// ---------- trace renderers ----------

pub fn render_trace_summary(s: &trace::summary::TraceSummary) -> String {
    let mut out = String::new();
    heading(&mut out, 1, &format!("Trace — {}", s.file_name));

    let _ = writeln!(out, "File:         {}", fmt_bytes(s.file_size));
    let _ = writeln!(out, "Duration:     {}", fmt_duration_us(s.duration_us));
    let _ = writeln!(out, "Events:       {}", fmt_num(s.event_count as u64));
    let _ = writeln!(out, "Processes:    {}", s.processes.len());
    let _ = writeln!(
        out,
        "Main thread:  {} (pid {})",
        s.main_thread_name, s.main_thread_pid
    );

    heading(&mut out, 2, "Processes");
    let mut t = Table::new(
        &["PID", "Name", "Threads", "Events"],
        vec![Align::Right, Align::Left, Align::Right, Align::Right],
    );
    for p in &s.processes {
        t.row(vec![
            p.pid.to_string(),
            p.name.clone(),
            p.thread_count.to_string(),
            fmt_num(p.event_count as u64),
        ]);
    }
    t.render(&mut out);

    heading(&mut out, 2, "Events by category");
    let mut t = Table::new(
        &["Category", "Count"],
        vec![Align::Left, Align::Right],
    );
    for (cat, count) in s.category_histogram.iter().take(15) {
        t.row(vec![truncate(cat, 60), fmt_num(*count as u64)]);
    }
    t.render(&mut out);

    heading(&mut out, 2, "Long tasks (>50ms)");
    if s.long_tasks.is_empty() {
        out.push_str("None.\n");
    } else {
        let mut t = Table::new(
            &["#", "Name", "Duration", "Offset", "Thread"],
            vec![Align::Right, Align::Left, Align::Right, Align::Right, Align::Left],
        );
        for (i, lt) in s.long_tasks.iter().enumerate() {
            t.row(vec![
                (i + 1).to_string(),
                lt.name.clone(),
                fmt_ms(lt.dur_ms),
                format!("+{}", fmt_ms(lt.ts_offset_ms)),
                lt.thread_name.clone(),
            ]);
        }
        t.render(&mut out);
    }

    out
}

#[allow(clippy::cast_precision_loss)]
pub fn render_trace_frames(f: &trace::frames::FrameAnalysis) -> String {
    let mut out = String::new();
    heading(
        &mut out,
        2,
        &format!(
            "Frame timing ({} frames over {})",
            fmt_num(f.frame_count as u64),
            fmt_ms(f.span_ms)
        ),
    );
    if f.frame_count == 0 {
        out.push_str("No AnimationFrame events found.\n");
        return out;
    }
    let total = f.frame_count.max(1);
    let _ = writeln!(out, "Avg FPS:             {:.1}", f.avg_fps);
    let _ = writeln!(
        out,
        "P50: {} | P95: {} | P99: {} | Max: {}",
        fmt_ms(f.p50_ms),
        fmt_ms(f.p95_ms),
        fmt_ms(f.p99_ms),
        fmt_ms(f.max_ms)
    );
    let _ = writeln!(
        out,
        "Jank (>16.67ms):     {} ({:.1}%)",
        f.jank_count,
        f.jank_count as f64 / total as f64 * 100.0
    );
    let _ = writeln!(out, "Severe (>50ms):      {}", f.severe_jank_count);
    out.push('\n');

    if !f.distribution.is_empty() {
        let mut t = Table::new(
            &["Range", "Count", "%"],
            vec![Align::Left, Align::Right, Align::Right],
        );
        for b in &f.distribution {
            let pct = b.count as f64 / total as f64 * 100.0;
            t.row(vec![
                b.label.clone(),
                fmt_num(b.count as u64),
                format!("{pct:.1}%"),
            ]);
        }
        t.render(&mut out);
    }

    if !f.worst_frames.is_empty() {
        heading(
            &mut out,
            3,
            &format!("Worst {} frames", f.worst_frames.len()),
        );
        for fr in &f.worst_frames {
            let _ = writeln!(
                out,
                "#{} {} at +{}",
                fr.rank,
                fmt_ms(fr.dur_ms),
                fmt_ms(fr.ts_offset_ms)
            );
            if fr.breakdown.is_empty() {
                out.push_str("  (no matching timeline events)\n\n");
            } else {
                let mut t = Table::new(
                    &["Event", "Duration", "Source"],
                    vec![Align::Left, Align::Right, Align::Left],
                );
                for ev in &fr.breakdown {
                    let source = format_source(&ev.function_name, &ev.url);
                    t.row(vec![ev.name.clone(), fmt_ms(ev.dur_ms), source]);
                }
                t.render(&mut out);
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
        (false, true) => func.to_owned(),
        (false, false) => format!("{func} ({url})"),
    }
}

pub fn render_trace_gc(g: &trace::gc::GcAnalysis) -> String {
    let mut out = String::new();
    heading(&mut out, 2, "GC pressure");
    let _ = writeln!(
        out,
        "Total GC: {} ({:.1}% of trace)",
        fmt_ms(g.total_gc_time_ms),
        g.gc_pct_of_trace
    );
    out.push('\n');

    let mut t = Table::new(
        &["Type", "Count", "Total", "Avg", "Max"],
        vec![Align::Left, Align::Right, Align::Right, Align::Right, Align::Right],
    );
    let buckets: &[(&str, &trace::gc::GcBucket)] = &[
        ("MajorGC", &g.major_gc),
        ("MinorGC", &g.minor_gc),
        ("Incremental marking", &g.incremental),
    ];
    for &(name, b) in buckets {
        if b.count > 0 {
            t.row(vec![
                name.to_owned(),
                b.count.to_string(),
                fmt_ms(b.total_ms),
                fmt_ms(b.avg_ms),
                fmt_ms(b.max_ms),
            ]);
        }
    }
    t.render(&mut out);

    if !g.top_events.is_empty() {
        heading(
            &mut out,
            3,
            &format!("Top {} GC events", g.top_events.len()),
        );
        let mut t = Table::new(
            &["#", "Type", "Duration", "Offset"],
            vec![Align::Right, Align::Left, Align::Right, Align::Right],
        );
        for (i, ev) in g.top_events.iter().enumerate() {
            t.row(vec![
                (i + 1).to_string(),
                ev.name.clone(),
                fmt_ms(ev.dur_ms),
                format!("+{}", fmt_ms(ev.ts_offset_ms)),
            ]);
        }
        t.render(&mut out);
    }

    out
}

pub fn render_trace_hotspots(h: &trace::hotspots::HotspotAnalysis) -> String {
    let mut out = String::new();

    if !h.by_url.is_empty() {
        heading(
            &mut out,
            2,
            &format!("JS execution by URL (top {})", h.by_url.len()),
        );
        let mut t = Table::new(
            &["URL", "Calls", "Total", "Avg", "Max"],
            vec![Align::Left, Align::Right, Align::Right, Align::Right, Align::Right],
        );
        for u in &h.by_url {
            t.row(vec![
                u.url.clone(),
                u.call_count.to_string(),
                fmt_ms(u.total_ms),
                fmt_ms(u.avg_ms),
                fmt_ms(u.max_ms),
            ]);
        }
        t.render(&mut out);
    }

    if !h.by_function.is_empty() {
        heading(
            &mut out,
            2,
            &format!("Top {} functions by total time", h.by_function.len()),
        );
        let mut t = Table::new(
            &["Function", "URL:line", "Calls", "Total", "Max"],
            vec![Align::Left, Align::Left, Align::Right, Align::Right, Align::Right],
        );
        for f in &h.by_function {
            let loc = if f.line > 0 {
                format!("{}:{}", f.url, f.line)
            } else {
                f.url.clone()
            };
            t.row(vec![
                f.function_name.clone(),
                loc,
                f.call_count.to_string(),
                fmt_ms(f.total_ms),
                fmt_ms(f.max_ms),
            ]);
        }
        t.render(&mut out);
    }

    if h.by_url.is_empty() && h.by_function.is_empty() {
        heading(&mut out, 2, "JS execution hotspots");
        out.push_str("No FunctionCall/EvaluateScript events found.\n");
    }

    out
}

// ---------- lighthouse renderer ----------

#[allow(clippy::too_many_lines)]
pub fn render_lighthouse(r: &lighthouse::LighthouseReport) -> String {
    let mut out = String::new();
    heading(&mut out, 1, &format!("Lighthouse — {}", r.file_name));

    let _ = writeln!(out, "URL:          {}", r.url);
    if r.final_url != r.url && !r.final_url.is_empty() {
        let _ = writeln!(out, "Final URL:    {}", r.final_url);
    }
    let _ = writeln!(out, "Fetched:      {}", r.fetch_time);
    let _ = writeln!(out, "Lighthouse:   v{}", r.lighthouse_version);
    let _ = writeln!(out, "Device:       {} ({})", r.form_factor, r.throttling.method);
    let _ = writeln!(
        out,
        "Throttling:   RTT {}ms, {:.0} Kbps, {}x CPU slowdown",
        r.throttling.rtt_ms, r.throttling.throughput_kbps, r.throttling.cpu_slowdown
    );
    let _ = writeln!(out, "File:         {}", fmt_bytes(r.file_size));

    if let Some(err) = &r.runtime_error {
        out.push('\n');
        let _ = writeln!(out, "!! RUNTIME ERROR: {} — {}", err.code, err.message);
    }
    for w in &r.run_warnings {
        let _ = writeln!(out, "!! WARNING: {w}");
    }

    // Category scores.
    if r.categories.iter().any(|c| c.score.is_some()) {
        heading(&mut out, 2, "Scores");
        let mut t = Table::new(
            &["Category", "Score"],
            vec![Align::Left, Align::Right],
        );
        for cat in &r.categories {
            let score = cat.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            t.row(vec![cat.title.clone(), score]);
        }
        t.render(&mut out);
    }

    // Metrics.
    let has_metrics = r.metrics.iter().any(|m| m.numeric_value.is_some());
    if has_metrics {
        heading(&mut out, 2, "Core Web Vitals / metrics");
        let mut t = Table::new(
            &["Metric", "Value", "Score"],
            vec![Align::Left, Align::Right, Align::Right],
        );
        for m in &r.metrics {
            let val = if m.display_value.is_empty() {
                m.numeric_value.map_or_else(|| "n/a".to_owned(), |v| format_metric_value(v, &m.numeric_unit))
            } else {
                m.display_value.clone()
            };
            let score = m.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            t.row(vec![m.title.clone(), val, score]);
        }
        t.render(&mut out);
    }

    // Diagnostics.
    for diag in &r.diagnostics {
        let title = if diag.display_value.is_empty() {
            diag.title.clone()
        } else {
            format!("{} ({})", diag.title, diag.display_value)
        };
        heading(&mut out, 2, &title);
        let mut t = Table::new(
            &["Item", "Value"],
            vec![Align::Left, Align::Right],
        );
        for row in &diag.details {
            t.row(vec![row.label.clone(), row.value.clone()]);
        }
        t.render(&mut out);
    }

    // Opportunities.
    if !r.opportunities.is_empty() {
        heading(&mut out, 2, "Opportunities");
        for opp in &r.opportunities {
            let savings = format_savings(opp.wasted_ms, opp.wasted_bytes);
            let _ = writeln!(out, "{} {}", opp.title, savings);
            if !opp.items.is_empty() {
                let mut t = Table::new(
                    &["URL", "Wasted"],
                    vec![Align::Left, Align::Right],
                );
                for item in &opp.items {
                    let wasted = format_item_savings(item.wasted_ms, item.wasted_bytes);
                    t.row(vec![item.url.clone(), wasted]);
                }
                t.render(&mut out);
            }
            out.push('\n');
        }
    }

    // Failed audits.
    if !r.failed_audits.is_empty() {
        heading(
            &mut out,
            2,
            &format!(
                "Failed audits ({} failed, {} passed)",
                r.failed_audits.len(),
                r.passed_audits
            ),
        );
        let mut t = Table::new(
            &["Category", "Audit", "Score"],
            vec![Align::Left, Align::Left, Align::Right],
        );
        for fa in &r.failed_audits {
            let score = fa.score.map_or_else(|| "n/a".to_owned(), |s| format!("{:.0}", s * 100.0));
            t.row(vec![fa.category.clone(), fa.title.clone(), score]);
        }
        t.render(&mut out);
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
