pub mod markdown;
pub mod text;

use humansize::{DECIMAL, format_size};

// ---------- shared formatting helpers ----------

pub fn fmt_bytes(b: u64) -> String {
    format_size(b, DECIMAL)
}

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count.is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

pub fn fmt_signed(n: i64) -> String {
    if n >= 0 {
        format!("+{}", fmt_num(n.cast_unsigned()))
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let abs = (-i128::from(n)) as u64;
        format!("-{}", fmt_num(abs))
    }
}

pub fn fmt_delta_bytes(d: i128) -> String {
    if d >= 0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = d as u64;
        format!("+{}", fmt_bytes(v))
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = (-d) as u64;
        format!("-{}", fmt_bytes(v))
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    let mut count = 0usize;
    let mut end = 0usize;
    for (i, c) in s.char_indices() {
        if count >= max {
            end = i;
            break;
        }
        count += 1;
        end = i + c.len_utf8();
    }
    if count < max || end == s.len() {
        s.to_owned()
    } else {
        let mut t: String = s[..end].to_owned();
        // Strip last char so the ellipsis fits within `max`.
        if let Some(last) = t.pop() {
            let _ = last;
        }
        t.push('\u{2026}');
        t
    }
}

pub fn sanitize_name(s: &str) -> String {
    // Chrome often stores empty or newline-bearing strings; make them readable.
    let s = s.replace(['\n', '\r', '\t'], " ");
    if s.is_empty() { "<empty>".to_owned() } else { s }
}

pub fn fmt_ms(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{ms:.2}ms")
    } else {
        format!("{:.0}us", ms * 1000.0)
    }
}

#[allow(clippy::cast_precision_loss)]
pub fn fmt_duration_us(us: u64) -> String {
    let ms = us as f64 / 1000.0;
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else {
        format!("{ms:.2}ms")
    }
}
