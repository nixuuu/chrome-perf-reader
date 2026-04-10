//! Raw serde structures mirroring the V8 `.heapsnapshot` JSON layout.
//!
//! The file is decoded in one pass into these structs, then transformed
//! into the richer `HeapGraph` in `graph.rs`.

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct RawSnapshot {
    pub snapshot: SnapshotHeader,
    pub nodes: Vec<u64>,
    pub edges: Vec<u32>,
    pub strings: Vec<String>,
}

#[derive(Deserialize)]
pub struct SnapshotHeader {
    pub meta: Meta,
}

/// Meta describes how the flat `nodes` / `edges` arrays are packed.
///
/// `node_types` and `edge_types` are heterogeneous: the first element is
/// itself an array of human-readable type names; subsequent elements are
/// strings describing the type of each remaining field. We only care
/// about the first element, so we store the whole thing as `Value` and
/// pull it apart in `graph.rs`.
#[derive(Deserialize)]
pub struct Meta {
    pub node_fields: Vec<String>,
    pub node_types: Vec<Value>,
    pub edge_fields: Vec<String>,
    pub edge_types: Vec<Value>,
}
