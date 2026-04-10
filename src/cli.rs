use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "chrome-perf-reader",
    version,
    about = "Parse Chrome/V8 .heapsnapshot and trace files, print human- and LLM-friendly analysis"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,

    /// File to analyze. Auto-detects format (heapsnapshot or trace).
    #[arg(value_name = "FILE", global = false)]
    pub file: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Markdown, global = true)]
    pub format: Format,

    /// Rows to show in top / detached / diff / hotspot tables.
    #[arg(long, default_value_t = 20, global = true)]
    pub limit: usize,

    /// Long task threshold in milliseconds (for trace analysis).
    #[arg(long, default_value_t = 50.0, global = true)]
    pub threshold: f64,
}

#[derive(Subcommand)]
pub enum Command {
    // ---- heap snapshot ----
    /// Heap: summary + type histogram.
    Summary {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Heap: top-N retainers.
    Top {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = SortBy::Retained)]
        by: SortBy,
    },
    /// Heap: detached candidates.
    Detached {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Heap: compare two snapshots.
    Diff {
        #[arg(value_name = "A")]
        a: PathBuf,
        #[arg(value_name = "B")]
        b: PathBuf,
    },

    // ---- trace ----
    /// Trace: full report (summary + frames + gc + hotspots).
    Trace {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Trace: frame timing / jank analysis.
    Frames {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Trace: GC pressure analysis.
    Gc {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Trace: JS execution hotspots.
    Hotspots {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Format {
    Markdown,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SortBy {
    #[value(name = "self")]
    SelfSize,
    #[value(name = "retained")]
    Retained,
    #[value(name = "both")]
    Both,
}
