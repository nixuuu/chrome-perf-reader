use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

/// Parsed Lighthouse report, ready for rendering.
pub struct LighthouseReport {
    pub file_name: String,
    pub file_size: u64,
    pub url: String,
    pub final_url: String,
    pub fetch_time: String,
    pub lighthouse_version: String,
    pub form_factor: String,
    pub throttling: ThrottlingConfig,
    pub runtime_error: Option<RuntimeError>,
    pub run_warnings: Vec<String>,
    pub categories: Vec<CategoryScore>,
    pub metrics: Vec<Metric>,
    pub diagnostics: Vec<DiagnosticItem>,
    pub opportunities: Vec<Opportunity>,
    pub passed_audits: usize,
    pub failed_audits: Vec<FailedAudit>,
}

pub struct ThrottlingConfig {
    pub rtt_ms: f64,
    pub throughput_kbps: f64,
    pub cpu_slowdown: f64,
    pub method: String,
}

pub struct RuntimeError {
    pub code: String,
    pub message: String,
}

pub struct CategoryScore {
    pub title: String,
    pub score: Option<f64>,
}

pub struct Metric {
    pub title: String,
    pub score: Option<f64>,
    pub numeric_value: Option<f64>,
    pub numeric_unit: String,
    pub display_value: String,
}

pub struct DiagnosticItem {
    pub title: String,
    pub display_value: String,
    pub details: Vec<DiagnosticRow>,
}

pub struct DiagnosticRow {
    pub label: String,
    pub value: String,
}

pub struct Opportunity {
    pub title: String,
    pub wasted_bytes: Option<f64>,
    pub wasted_ms: Option<f64>,
    pub items: Vec<OpportunityItem>,
}

pub struct OpportunityItem {
    pub url: String,
    pub wasted_bytes: Option<f64>,
    pub wasted_ms: Option<f64>,
}

pub struct FailedAudit {
    pub title: String,
    pub category: String,
    pub score: Option<f64>,
}

/// Well-known metric audit IDs.
const METRIC_IDS: &[&str] = &[
    "first-contentful-paint",
    "largest-contentful-paint",
    "speed-index",
    "total-blocking-time",
    "cumulative-layout-shift",
    "interactive",
    "max-potential-fid",
    "server-response-time",
];

/// Audits that produce table-like diagnostic info.
const DIAGNOSTIC_IDS: &[&str] = &[
    "mainthread-work-breakdown",
    "bootup-time",
    "dom-size",
    "third-party-summary",
    "resource-summary",
    "total-byte-weight",
    "diagnostics",
];

/// Audits that suggest optimization opportunities with byte/ms savings.
const OPPORTUNITY_IDS: &[&str] = &[
    "render-blocking-resources",
    "unused-javascript",
    "unused-css-rules",
    "offscreen-images",
    "uses-text-compression",
    "uses-responsive-images",
    "uses-optimized-images",
    "uses-rel-preconnect",
    "unminified-css",
    "unminified-javascript",
    "uses-long-cache-ttl",
    "duplicated-javascript",
    "legacy-javascript",
];

/// Categories whose failed audits (score < 1) we list.
const FAIL_CATEGORIES: &[&str] = &["accessibility", "best-practices", "seo"];

impl LighthouseReport {
    pub fn load(path: &Path) -> Result<Self> {
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_owned();
        let file_size = std::fs::metadata(path)
            .with_context(|| format!("stat {}", path.display()))?
            .len();

        let raw = std::fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let root: Value = serde_json::from_slice(&raw)
            .with_context(|| format!("parsing JSON {}", path.display()))?;

        Ok(Self::parse(&root, file_name, file_size))
    }

    fn parse(root: &Value, file_name: String, file_size: u64) -> Self {
        let url = str_field(root, "requestedUrl");
        let final_url = str_field(root, "finalDisplayedUrl");
        let fetch_time = str_field(root, "fetchTime");
        let lighthouse_version = str_field(root, "lighthouseVersion");

        let config = &root["configSettings"];
        let form_factor = str_field(config, "formFactor");

        let thr = &config["throttling"];
        let throttling = ThrottlingConfig {
            rtt_ms: thr["rttMs"].as_f64().unwrap_or(0.0),
            throughput_kbps: thr["throughputKbps"].as_f64().unwrap_or(0.0),
            cpu_slowdown: thr["cpuSlowdownMultiplier"].as_f64().unwrap_or(1.0),
            method: str_field(config, "throttlingMethod"),
        };

        let runtime_error = {
            let re = &root["runtimeError"];
            let code = str_field(re, "code");
            if !code.is_empty() && code != "NO_ERROR" {
                Some(RuntimeError {
                    code,
                    message: str_field(re, "message"),
                })
            } else {
                None
            }
        };

        let run_warnings: Vec<String> = root["runWarnings"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        let categories = parse_categories(&root["categories"]);
        let audits = &root["audits"];
        let metrics = parse_metrics(audits);
        let diagnostics = parse_diagnostics(audits);
        let opportunities = parse_opportunities(audits);
        let (passed_audits, failed_audits) =
            parse_failed_audits(&root["categories"], audits);

        Self {
            file_name,
            file_size,
            url,
            final_url,
            fetch_time,
            lighthouse_version,
            form_factor,
            throttling,
            runtime_error,
            run_warnings,
            categories,
            metrics,
            diagnostics,
            opportunities,
            passed_audits,
            failed_audits,
        }
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_owned()
}

fn parse_categories(cats: &Value) -> Vec<CategoryScore> {
    let Some(obj) = cats.as_object() else {
        return Vec::new();
    };
    // Stable order: performance, accessibility, best-practices, seo.
    let order = ["performance", "accessibility", "best-practices", "seo"];
    let mut out = Vec::new();
    for id in &order {
        if let Some(cat) = obj.get(*id) {
            out.push(CategoryScore {
                title: str_field(cat, "title"),
                score: cat["score"].as_f64(),
            });
        }
    }
    // Any remaining.
    for (id, cat) in obj {
        if !order.contains(&id.as_str()) {
            out.push(CategoryScore {
                title: str_field(cat, "title"),
                score: cat["score"].as_f64(),
            });
        }
    }
    out
}

fn parse_metrics(audits: &Value) -> Vec<Metric> {
    METRIC_IDS
        .iter()
        .filter_map(|&id| {
            let a = &audits[id];
            if a.is_null() {
                return None;
            }
            Some(Metric {
                title: str_field(a, "title"),
                score: a["score"].as_f64(),
                numeric_value: a["numericValue"].as_f64(),
                numeric_unit: str_field(a, "numericUnit"),
                display_value: str_field(a, "displayValue"),
            })
        })
        .collect()
}

fn parse_diagnostics(audits: &Value) -> Vec<DiagnosticItem> {
    let mut out = Vec::new();
    for &id in DIAGNOSTIC_IDS {
        let a = &audits[id];
        if a.is_null() {
            continue;
        }
        let items = &a["details"]["items"];
        let rows: Vec<DiagnosticRow> = match items.as_array() {
            Some(arr) if !arr.is_empty() => arr
                .iter()
                .take(15)
                .filter_map(|item| diagnostic_row(id, item))
                .collect(),
            _ => continue,
        };
        if rows.is_empty() {
            continue;
        }
        out.push(DiagnosticItem {
            title: str_field(a, "title"),
            display_value: str_field(a, "displayValue"),
            details: rows,
        });
    }
    out
}

fn diagnostic_row(audit_id: &str, item: &Value) -> Option<DiagnosticRow> {
    match audit_id {
        "mainthread-work-breakdown" => {
            let group = item["groupLabel"]
                .as_str()
                .or_else(|| item["group"].as_str())?;
            let dur = item["duration"].as_f64()?;
            Some(DiagnosticRow {
                label: group.to_owned(),
                value: format!("{dur:.0}ms"),
            })
        }
        "bootup-time" => {
            let url = item["url"].as_str()?;
            let total = item["total"].as_f64().unwrap_or(0.0);
            Some(DiagnosticRow {
                label: truncate_url(url),
                value: format!("{total:.0}ms"),
            })
        }
        "resource-summary" => {
            let rt = item["resourceType"].as_str().unwrap_or("?");
            let count = item["requestCount"].as_u64().unwrap_or(0);
            let size = item["transferSize"].as_u64().unwrap_or(0);
            Some(DiagnosticRow {
                label: rt.to_owned(),
                value: format!("{count} req, {}", fmt_transfer(size)),
            })
        }
        "third-party-summary" => {
            let entity = item["entity"]
                .as_str()
                .or_else(|| item["entity"]["text"].as_str())
                .unwrap_or("?");
            let size = item["transferSize"].as_u64().unwrap_or(0);
            let bt = item["blockingTime"].as_f64().unwrap_or(0.0);
            Some(DiagnosticRow {
                label: entity.to_owned(),
                value: format!("{}, blocking {bt:.0}ms", fmt_transfer(size)),
            })
        }
        "total-byte-weight" => {
            let url = item["url"].as_str()?;
            let size = item["totalBytes"].as_u64().unwrap_or(0);
            Some(DiagnosticRow {
                label: truncate_url(url),
                value: fmt_transfer(size),
            })
        }
        "dom-size" => {
            let stat = item["statistic"].as_str().unwrap_or("");
            let val = item["value"].as_f64().map(|v| format!("{v:.0}"));
            let node = item["node"].as_object().and_then(|n| {
                n.get("snippet").and_then(|s| s.as_str()).map(str::to_owned)
            });
            let display = val.or(node).unwrap_or_default();
            if stat.is_empty() && display.is_empty() {
                return None;
            }
            Some(DiagnosticRow {
                label: stat.to_owned(),
                value: display,
            })
        }
        "diagnostics" => {
            // Generic key-value from "diagnostics" audit.
            // Items have various fields; grab what we can.
            None
        }
        _ => None,
    }
}

fn parse_opportunities(audits: &Value) -> Vec<Opportunity> {
    let mut out: Vec<Opportunity> = Vec::new();
    for &id in OPPORTUNITY_IDS {
        let a = &audits[id];
        if a.is_null() {
            continue;
        }
        // Only include if there's actual savings.
        let details = &a["details"];
        let overall_savings_ms = details["overallSavingsMs"].as_f64();
        let overall_savings_bytes = details["overallSavingsBytes"].as_f64();

        let has_savings = overall_savings_ms.is_some_and(|v| v > 0.0)
            || overall_savings_bytes.is_some_and(|v| v > 0.0);
        // Also include if score < 1 (audit flagged it).
        let flagged = a["score"].as_f64().is_some_and(|s| s < 1.0);
        if !has_savings && !flagged {
            continue;
        }

        let items: Vec<OpportunityItem> = details["items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .take(10)
                    .filter_map(|item| {
                        let url = item["url"].as_str().unwrap_or("");
                        if url.is_empty() {
                            return None;
                        }
                        Some(OpportunityItem {
                            url: truncate_url(url),
                            wasted_bytes: item["wastedBytes"].as_f64(),
                            wasted_ms: item["wastedMs"].as_f64(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        out.push(Opportunity {
            title: str_field(a, "title"),
            wasted_bytes: overall_savings_bytes,
            wasted_ms: overall_savings_ms,
            items,
        });
    }
    // Sort by wasted ms desc, then wasted bytes desc.
    out.sort_by(|a, b| {
        let ms_a = a.wasted_ms.unwrap_or(0.0);
        let ms_b = b.wasted_ms.unwrap_or(0.0);
        ms_b.partial_cmp(&ms_a)
            .unwrap()
            .then_with(|| {
                let ba = a.wasted_bytes.unwrap_or(0.0);
                let bb = b.wasted_bytes.unwrap_or(0.0);
                bb.partial_cmp(&ba).unwrap()
            })
    });
    out
}

fn parse_failed_audits(categories: &Value, audits: &Value) -> (usize, Vec<FailedAudit>) {
    let Some(cats) = categories.as_object() else {
        return (0, Vec::new());
    };
    let mut passed = 0usize;
    let mut failed = Vec::new();

    for &cat_id in FAIL_CATEGORIES {
        let Some(cat) = cats.get(cat_id) else {
            continue;
        };
        let cat_title = str_field(cat, "title");
        let Some(refs) = cat["auditRefs"].as_array() else {
            continue;
        };
        for r in refs {
            let Some(audit_id) = r["id"].as_str() else {
                continue;
            };
            let a = &audits[audit_id];
            if a.is_null() {
                continue;
            }
            match a["score"].as_f64() {
                Some(s) if s >= 1.0 => {
                    passed += 1;
                }
                Some(s) => {
                    failed.push(FailedAudit {
                        title: str_field(a, "title"),
                        category: cat_title.clone(),
                        score: Some(s),
                    });
                }
                None => {
                    // null score = error or not applicable, skip.
                }
            }
        }
    }
    // Sort by score ascending (worst first).
    failed.sort_by(|a, b| {
        a.score
            .unwrap_or(0.0)
            .partial_cmp(&b.score.unwrap_or(0.0))
            .unwrap()
    });
    (passed, failed)
}

#[allow(clippy::cast_precision_loss)]
fn fmt_transfer(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        let mb = bytes as f64 / 1_048_576.0;
        format!("{mb:.1} MB")
    } else if bytes >= 1024 {
        let kb = bytes as f64 / 1024.0;
        format!("{kb:.1} KB")
    } else {
        format!("{bytes} B")
    }
}

/// Truncate a URL to a reasonable display length.
/// Keeps the domain + first path segment, and last segment (filename).
fn truncate_url(url: &str) -> String {
    if url.len() <= 120 {
        return url.to_owned();
    }
    // Try to keep domain + beginning and end of path.
    // Find the path portion after scheme+host.
    let path_start = url.find("://").map_or(0, |i| {
        url[i + 3..].find('/').map_or(url.len(), |j| i + 3 + j)
    });

    let prefix = &url[..path_start.min(url.len())];
    let path = &url[path_start..];

    if path.len() <= 60 {
        return url.to_owned();
    }

    // Show first 40 chars of path + ... + last 40 chars
    let path_bytes: Vec<char> = path.chars().collect();
    let head: String = path_bytes[..40].iter().collect();
    let tail: String = path_bytes[path_bytes.len().saturating_sub(40)..].iter().collect();
    format!("{prefix}{head}...{tail}")
}

/// Peek at raw file bytes to check if this looks like a Lighthouse report.
pub fn looks_like_lighthouse(path: &Path) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    // Check first ~1KB for lighthouse marker.
    let prefix = &raw[..raw.len().min(1024)];
    let s = String::from_utf8_lossy(prefix);
    s.contains("lighthouseVersion")
}
