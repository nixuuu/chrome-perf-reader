use std::collections::{BTreeMap, HashMap, HashSet};

use crate::trace::parser::TraceFile;

pub struct TraceSummary {
    pub file_name: String,
    pub file_size: u64,
    pub event_count: usize,
    pub duration_us: u64,
    pub main_thread_name: String,
    pub main_thread_pid: u64,
    pub processes: Vec<ProcessInfo>,
    pub category_histogram: Vec<(String, usize)>,
    pub long_tasks: Vec<LongTask>,
}

pub struct ProcessInfo {
    pub pid: u64,
    pub name: String,
    pub thread_count: usize,
    pub event_count: usize,
}

pub struct LongTask {
    pub name: String,
    pub dur_ms: f64,
    pub ts_offset_ms: f64,
    pub thread_name: String,
}

pub fn compute(
    trace: &TraceFile,
    file_name: String,
    file_size: u64,
    threshold_ms: f64,
) -> TraceSummary {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let threshold_micros = (threshold_ms * 1000.0) as u64;

    // Category histogram.
    let mut cat_counts: BTreeMap<String, usize> = BTreeMap::new();
    for ev in &trace.events {
        if ev.cat.is_empty() {
            continue;
        }
        *cat_counts.entry(ev.cat.clone()).or_insert(0) += 1;
    }
    let mut category_histogram: Vec<(String, usize)> = cat_counts.into_iter().collect();
    category_histogram.sort_by(|a, b| b.1.cmp(&a.1));

    // Process info.
    let mut proc_threads: HashMap<u64, HashSet<u64>> = HashMap::new();
    let mut proc_events: HashMap<u64, usize> = HashMap::new();
    for ev in &trace.events {
        proc_threads
            .entry(ev.pid)
            .or_default()
            .insert(ev.tid);
        *proc_events.entry(ev.pid).or_insert(0) += 1;
    }
    let mut processes: Vec<ProcessInfo> = proc_threads
        .into_iter()
        .map(|(pid, threads)| ProcessInfo {
            pid,
            name: trace
                .process_names
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| format!("pid {pid}")),
            thread_count: threads.len(),
            event_count: *proc_events.get(&pid).unwrap_or(&0),
        })
        .collect();
    processes.sort_by(|a, b| b.event_count.cmp(&a.event_count));

    // Long tasks: complete events (ph=X) on any thread exceeding threshold.
    let mut long_tasks: Vec<LongTask> = Vec::new();
    for ev in &trace.events {
        if ev.ph != "X" {
            continue;
        }
        let Some(dur) = ev.dur else { continue };
        if dur < threshold_micros {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        long_tasks.push(LongTask {
            name: ev.name.clone(),
            dur_ms: dur as f64 / 1000.0,
            ts_offset_ms: ev.ts.saturating_sub(trace.min_ts) as f64 / 1000.0,
            thread_name: trace.thread_name(ev.pid, ev.tid).to_owned(),
        });
    }
    long_tasks.sort_by(|a, b| b.dur_ms.partial_cmp(&a.dur_ms).unwrap());

    let main_thread_name = trace
        .main_thread
        .map_or_else(|| "unknown".to_owned(), |(p, t)| trace.thread_name(p, t).to_owned());
    let main_thread_pid = trace.main_thread.map_or(0, |(p, _)| p);

    TraceSummary {
        file_name,
        file_size,
        event_count: trace.events.len(),
        duration_us: trace.duration_us,
        main_thread_name,
        main_thread_pid,
        processes,
        category_histogram,
        long_tasks,
    }
}
