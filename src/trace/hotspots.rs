use std::collections::HashMap;

use crate::trace::parser::TraceFile;

pub struct HotspotAnalysis {
    pub by_url: Vec<UrlBucket>,
    pub by_function: Vec<FunctionBucket>,
}

pub struct UrlBucket {
    pub url: String,
    pub call_count: usize,
    pub total_ms: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
}

pub struct FunctionBucket {
    pub function_name: String,
    pub url: String,
    pub line: u32,
    pub call_count: usize,
    pub total_ms: f64,
    pub max_ms: f64,
}

pub fn compute(trace: &TraceFile, limit: usize) -> HotspotAnalysis {
    // Aggregate FunctionCall + EvaluateScript events.
    let mut url_agg: HashMap<String, (usize, f64, f64)> = HashMap::new(); // count, total, max
    let mut fn_agg: HashMap<(String, String, u32), (usize, f64, f64)> = HashMap::new();

    for ev in &trace.events {
        if ev.ph != "X" {
            continue;
        }
        if ev.name != "FunctionCall" && ev.name != "EvaluateScript" {
            continue;
        }
        let dur_ms = match ev.dur {
            Some(d) => d as f64 / 1000.0,
            None => continue,
        };

        let args = match &ev.args {
            Some(a) => a,
            None => continue,
        };
        let data = match args.get("data") {
            Some(d) => d,
            None => continue,
        };

        let url = data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if url.is_empty() {
            continue;
        }

        let fn_name = data
            .get("functionName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let line = data
            .get("lineNumber")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // URL aggregation.
        let entry = url_agg.entry(url.clone()).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += dur_ms;
        if dur_ms > entry.2 {
            entry.2 = dur_ms;
        }

        // Function aggregation (only if function name present).
        if !fn_name.is_empty() {
            let key = (url, fn_name, line);
            let entry = fn_agg.entry(key).or_insert((0, 0.0, 0.0));
            entry.0 += 1;
            entry.1 += dur_ms;
            if dur_ms > entry.2 {
                entry.2 = dur_ms;
            }
        }
    }

    let mut by_url: Vec<UrlBucket> = url_agg
        .into_iter()
        .map(|(url, (count, total, max))| UrlBucket {
            url,
            call_count: count,
            total_ms: total,
            avg_ms: if count > 0 {
                total / count as f64
            } else {
                0.0
            },
            max_ms: max,
        })
        .collect();
    by_url.sort_by(|a, b| b.total_ms.partial_cmp(&a.total_ms).unwrap());
    by_url.truncate(limit);

    let mut by_function: Vec<FunctionBucket> = fn_agg
        .into_iter()
        .map(|((url, function_name, line), (count, total, max))| FunctionBucket {
            function_name,
            url,
            line,
            call_count: count,
            total_ms: total,
            max_ms: max,
        })
        .collect();
    by_function.sort_by(|a, b| b.total_ms.partial_cmp(&a.total_ms).unwrap());
    by_function.truncate(limit);

    HotspotAnalysis {
        by_url,
        by_function,
    }
}
