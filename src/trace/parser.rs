use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TraceEvent {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cat: String,
    #[serde(default)]
    pub ph: String,
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub dur: Option<u64>,
    #[serde(default)]
    pub pid: u64,
    #[serde(default)]
    pub tid: u64,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

pub struct TraceFile {
    pub events: Vec<TraceEvent>,
    /// (pid, tid) → thread name from metadata events.
    pub thread_names: HashMap<(u64, u64), String>,
    /// pid → process name (from process_name or thread "CrBrowserMain" etc).
    pub process_names: HashMap<u64, String>,
    /// Detected renderer main thread (pid, tid).
    pub main_thread: Option<(u64, u64)>,
    /// Trace duration in microseconds (max_ts - min_ts of events with ts > 0).
    pub duration_us: u64,
    /// Earliest timestamp.
    pub min_ts: u64,
}

/// Wrapper for deserializing the object-format trace.
#[derive(Deserialize)]
struct TraceWrapper {
    #[serde(rename = "traceEvents")]
    trace_events: Vec<TraceEvent>,
}

impl TraceFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;

        let bytes = decompress_if_gzip(raw)?;
        let events = parse_events(&bytes)
            .with_context(|| format!("parsing trace {}", path.display()))?;

        Self::build(events)
    }

    fn build(events: Vec<TraceEvent>) -> Result<Self> {
        let mut thread_names: HashMap<(u64, u64), String> = HashMap::new();
        let mut process_names: HashMap<u64, String> = HashMap::new();

        for ev in &events {
            if ev.ph != "M" {
                continue;
            }
            if let Some(args) = &ev.args {
                if ev.name == "thread_name" {
                    if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                        thread_names.insert((ev.pid, ev.tid), n.to_owned());
                    }
                } else if ev.name == "process_name" {
                    if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                        process_names.insert(ev.pid, n.to_owned());
                    }
                }
            }
        }

        // Detect main thread: prefer CrRendererMain, fall back to CrBrowserMain.
        let main_thread = thread_names
            .iter()
            .find(|(_, name)| name.as_str() == "CrRendererMain")
            .or_else(|| {
                thread_names
                    .iter()
                    .find(|(_, name)| name.as_str() == "CrBrowserMain")
            })
            .map(|(k, _)| *k);

        // Duration from non-zero timestamps.
        let mut min_ts = u64::MAX;
        let mut max_ts = 0u64;
        for ev in &events {
            if ev.ts == 0 {
                continue;
            }
            if ev.ts < min_ts {
                min_ts = ev.ts;
            }
            let end = ev.ts + ev.dur.unwrap_or(0);
            if end > max_ts {
                max_ts = end;
            }
        }
        if min_ts == u64::MAX {
            min_ts = 0;
        }
        let duration_us = max_ts.saturating_sub(min_ts);

        Ok(TraceFile {
            events,
            thread_names,
            process_names,
            main_thread,
            duration_us,
            min_ts,
        })
    }

    pub fn thread_name(&self, pid: u64, tid: u64) -> &str {
        self.thread_names
            .get(&(pid, tid))
            .map(String::as_str)
            .unwrap_or("?")
    }
}

fn decompress_if_gzip(raw: Vec<u8>) -> Result<Vec<u8>> {
    if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .context("decompressing gzip")?;
        Ok(out)
    } else {
        Ok(raw)
    }
}

fn parse_events(bytes: &[u8]) -> Result<Vec<TraceEvent>> {
    // Try object format first: {"traceEvents": [...]}
    if let Ok(wrapper) = serde_json::from_slice::<TraceWrapper>(bytes) {
        return Ok(wrapper.trace_events);
    }
    // Fall back to array format: [...]
    let events: Vec<TraceEvent> =
        serde_json::from_slice(bytes).context("not a valid trace file (expected object with traceEvents or array of events)")?;
    Ok(events)
}

/// Peek at raw file bytes to check if this looks like a Chrome trace.
/// Used for auto-detection in main.rs.
pub fn looks_like_trace(path: &Path) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    let Ok(bytes) = decompress_if_gzip(raw) else {
        return false;
    };
    // Check first ~4KB for trace markers. The "traceEvents" key often
    // appears after a large "metadata" block (~600+ bytes).
    let prefix = &bytes[..bytes.len().min(4096)];
    let s = String::from_utf8_lossy(prefix);
    s.contains("traceEvents") || s.contains("\"ph\"")
}
