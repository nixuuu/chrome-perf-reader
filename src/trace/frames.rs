use std::collections::HashMap;

use crate::trace::parser::TraceFile;

pub struct FrameAnalysis {
    pub frame_count: usize,
    pub span_ms: f64,
    pub avg_fps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub jank_count: usize,
    pub severe_jank_count: usize,
    pub distribution: Vec<FrameBucket>,
    pub worst_frames: Vec<FrameDetail>,
}

pub struct FrameBucket {
    pub label: String,
    pub count: usize,
}

pub struct FrameDetail {
    pub rank: usize,
    pub dur_ms: f64,
    pub ts_offset_ms: f64,
    /// What happened inside this frame, sorted by duration desc.
    pub breakdown: Vec<FrameEvent>,
}

pub struct FrameEvent {
    pub name: String,
    pub dur_ms: f64,
    /// For FunctionCall/EvaluateScript: source URL.
    pub url: String,
    /// For FunctionCall: function name.
    pub function_name: String,
}

const JANK_US: u64 = 16_667;
const SEVERE_US: u64 = 50_000;

/// Event names worth showing in frame breakdowns.
const BREAKDOWN_NAMES: &[&str] = &[
    "FunctionCall",
    "EvaluateScript",
    "Layout",
    "UpdateLayoutTree",
    "Paint",
    "PrePaint",
    "Layerize",
    "EventDispatch",
    "TimerFire",
    "FireIdleCallback",
    "FireAnimationFrame",
    "ScrollLayer",
    "MajorGC",
    "MinorGC",
    "HitTest",
    "ParseHTML",
];

pub fn compute(trace: &TraceFile, limit: usize) -> FrameAnalysis {
    // Collect frame time ranges: (begin_ts, end_ts).
    let mut frame_ranges: Vec<(u64, u64)> = Vec::new();

    // Strategy 1: Complete (X) AnimationFrame events.
    for ev in &trace.events {
        if ev.ph == "X" && ev.name == "AnimationFrame" {
            if let Some(dur) = ev.dur {
                frame_ranges.push((ev.ts, ev.ts + dur));
            }
        }
    }

    // Strategy 2: Pair async begin/end (b/e) AnimationFrame events.
    if frame_ranges.is_empty() {
        let mut begins: Vec<u64> = Vec::new();
        let mut ends: Vec<u64> = Vec::new();
        for ev in &trace.events {
            if ev.name != "AnimationFrame" {
                continue;
            }
            if ev.ph == "b" {
                begins.push(ev.ts);
            } else if ev.ph == "e" {
                ends.push(ev.ts);
            }
        }
        begins.sort_unstable();
        ends.sort_unstable();
        for (b, e) in begins.iter().zip(ends.iter()) {
            if *e > *b {
                frame_ranges.push((*b, *e));
            }
        }
    }

    let frame_count = frame_ranges.len();
    if frame_count == 0 {
        return empty();
    }

    let mut durations_us: Vec<u64> = frame_ranges
        .iter()
        .map(|(b, e)| e - b)
        .collect();
    durations_us.sort_unstable();

    let total_us: u64 = durations_us.iter().sum();
    let span_ms = total_us as f64 / 1000.0;

    let (first_ts, last_ts) = ts_range(&frame_ranges);
    let wall_ms = last_ts.saturating_sub(first_ts) as f64 / 1000.0;
    let avg_fps = if wall_ms > 0.0 {
        frame_count as f64 / (wall_ms / 1000.0)
    } else {
        0.0
    };

    let p50_ms = percentile(&durations_us, 50) as f64 / 1000.0;
    let p95_ms = percentile(&durations_us, 95) as f64 / 1000.0;
    let p99_ms = percentile(&durations_us, 99) as f64 / 1000.0;
    let max_ms = *durations_us.last().unwrap() as f64 / 1000.0;

    let jank_count = durations_us.iter().filter(|&&d| d > JANK_US).count();
    let severe_jank_count = durations_us.iter().filter(|&&d| d > SEVERE_US).count();

    let ranges: &[(u64, u64, &str)] = &[
        (0, 8_000, "0-8ms"),
        (8_000, 16_000, "8-16ms"),
        (16_000, 33_000, "16-33ms"),
        (33_000, 50_000, "33-50ms"),
        (50_000, u64::MAX, ">50ms"),
    ];
    let distribution: Vec<FrameBucket> = ranges
        .iter()
        .map(|&(lo, hi, label)| FrameBucket {
            label: label.to_owned(),
            count: durations_us.iter().filter(|&&d| d >= lo && d < hi).count(),
        })
        .collect();

    // Build worst frames WITH breakdown of internal events.
    let mut sorted_ranges = frame_ranges.clone();
    sorted_ranges.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)));
    sorted_ranges.truncate(limit);

    // Build a lookup set of breakdown event names for fast matching.
    let breakdown_set: HashMap<&str, ()> = BREAKDOWN_NAMES
        .iter()
        .map(|&n| (n, ()))
        .collect();

    // Collect candidate breakdown events (X events with relevant names on renderer main thread).
    let main_thread = trace.main_thread;
    let mut candidates: Vec<(&crate::trace::parser::TraceEvent, f64)> = Vec::new();
    for ev in &trace.events {
        if ev.ph != "X" {
            continue;
        }
        let dur = match ev.dur {
            Some(d) if d > 0 => d,
            _ => continue,
        };
        if !breakdown_set.contains_key(ev.name.as_str()) {
            continue;
        }
        // Only events on main thread (if known).
        if let Some((mp, mt)) = main_thread {
            if ev.pid != mp || ev.tid != mt {
                continue;
            }
        }
        candidates.push((ev, dur as f64 / 1000.0));
    }
    // Sort candidates by ts for binary search.
    candidates.sort_by_key(|(ev, _)| ev.ts);

    let worst_frames: Vec<FrameDetail> = sorted_ranges
        .iter()
        .enumerate()
        .map(|(i, &(begin, end))| {
            let dur_ms = (end - begin) as f64 / 1000.0;
            let ts_offset_ms = begin.saturating_sub(trace.min_ts) as f64 / 1000.0;

            // Binary search for the start position in candidates.
            let start_idx = candidates
                .partition_point(|(ev, _)| ev.ts < begin);

            let mut events: Vec<FrameEvent> = Vec::new();
            for &(ev, dur) in &candidates[start_idx..] {
                if ev.ts >= end {
                    break;
                }
                let (url, function_name) = extract_source(ev);
                events.push(FrameEvent {
                    name: ev.name.clone(),
                    dur_ms: dur,
                    url,
                    function_name,
                });
            }
            events.sort_by(|a, b| b.dur_ms.partial_cmp(&a.dur_ms).unwrap());
            events.truncate(10);

            FrameDetail {
                rank: i + 1,
                dur_ms,
                ts_offset_ms,
                breakdown: events,
            }
        })
        .collect();

    FrameAnalysis {
        frame_count,
        span_ms,
        avg_fps,
        p50_ms,
        p95_ms,
        p99_ms,
        max_ms,
        jank_count,
        severe_jank_count,
        distribution,
        worst_frames,
    }
}

fn extract_source(ev: &crate::trace::parser::TraceEvent) -> (String, String) {
    let args = match &ev.args {
        Some(a) => a,
        None => return (String::new(), String::new()),
    };
    let data = match args.get("data") {
        Some(d) => d,
        None => return (String::new(), String::new()),
    };
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let func = data
        .get("functionName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    (url, func)
}

fn ts_range(frames: &[(u64, u64)]) -> (u64, u64) {
    let mut min = u64::MAX;
    let mut max = 0u64;
    for &(b, _) in frames {
        if b < min { min = b; }
        if b > max { max = b; }
    }
    (min, max)
}

fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() { return 0; }
    let idx = (pct * sorted.len() / 100).min(sorted.len() - 1);
    sorted[idx]
}

fn empty() -> FrameAnalysis {
    FrameAnalysis {
        frame_count: 0,
        span_ms: 0.0,
        avg_fps: 0.0,
        p50_ms: 0.0,
        p95_ms: 0.0,
        p99_ms: 0.0,
        max_ms: 0.0,
        jank_count: 0,
        severe_jank_count: 0,
        distribution: Vec::new(),
        worst_frames: Vec::new(),
    }
}
