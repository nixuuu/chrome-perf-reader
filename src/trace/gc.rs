use crate::trace::parser::TraceFile;

pub struct GcAnalysis {
    pub major_gc: GcBucket,
    pub minor_gc: GcBucket,
    pub incremental: GcBucket,
    pub total_gc_time_ms: f64,
    pub gc_pct_of_trace: f64,
    pub top_events: Vec<GcEvent>,
}

pub struct GcBucket {
    pub count: usize,
    pub total_ms: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
}

pub struct GcEvent {
    pub name: String,
    pub dur_ms: f64,
    pub ts_offset_ms: f64,
}

impl GcBucket {
    const fn empty() -> Self {
        Self {
            count: 0,
            total_ms: 0.0,
            avg_ms: 0.0,
            max_ms: 0.0,
        }
    }

    fn from_durations(durs: &[f64]) -> Self {
        if durs.is_empty() {
            return Self::empty();
        }
        let total: f64 = durs.iter().sum();
        let max = durs.iter().copied().fold(0.0f64, f64::max);
        #[allow(clippy::cast_precision_loss)]
        let avg_ms = total / durs.len() as f64;
        Self {
            count: durs.len(),
            total_ms: total,
            avg_ms,
            max_ms: max,
        }
    }
}

pub fn compute(trace: &TraceFile, limit: usize) -> GcAnalysis {
    let mut major_durs: Vec<f64> = Vec::new();
    let mut minor_durs: Vec<f64> = Vec::new();
    let mut incr_durs: Vec<f64> = Vec::new();
    let mut all_gc: Vec<GcEvent> = Vec::new();

    for ev in &trace.events {
        if ev.ph != "X" {
            continue;
        }
        let Some(dur) = ev.dur else { continue };
        #[allow(clippy::cast_precision_loss)]
        let dur_ms = dur as f64 / 1000.0;

        match ev.name.as_str() {
            "MajorGC" => {
                major_durs.push(dur_ms);
                #[allow(clippy::cast_precision_loss)]
                let ts_offset_ms = ev.ts.saturating_sub(trace.min_ts) as f64 / 1000.0;
                all_gc.push(GcEvent {
                    name: ev.name.clone(),
                    dur_ms,
                    ts_offset_ms,
                });
            }
            "MinorGC" => {
                minor_durs.push(dur_ms);
                #[allow(clippy::cast_precision_loss)]
                let ts_offset_ms = ev.ts.saturating_sub(trace.min_ts) as f64 / 1000.0;
                all_gc.push(GcEvent {
                    name: ev.name.clone(),
                    dur_ms,
                    ts_offset_ms,
                });
            }
            _ => {}
        }

        if ev.name == "V8.GC_MC_INCREMENTAL" {
            incr_durs.push(dur_ms);
        }
    }

    let major_gc = GcBucket::from_durations(&major_durs);
    let minor_gc = GcBucket::from_durations(&minor_durs);
    let incremental = GcBucket::from_durations(&incr_durs);

    let total_gc_time_ms = major_gc.total_ms + minor_gc.total_ms + incremental.total_ms;
    #[allow(clippy::cast_precision_loss)]
    let trace_ms = trace.duration_us as f64 / 1000.0;
    let gc_pct_of_trace = if trace_ms > 0.0 {
        (total_gc_time_ms / trace_ms) * 100.0
    } else {
        0.0
    };

    all_gc.sort_by(|a, b| b.dur_ms.partial_cmp(&a.dur_ms).unwrap());
    all_gc.truncate(limit);

    GcAnalysis {
        major_gc,
        minor_gc,
        incremental,
        total_gc_time_ms,
        gc_pct_of_trace,
        top_events: all_gc,
    }
}
