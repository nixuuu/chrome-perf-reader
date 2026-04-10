//! Decoded, indexed heap graph ready for analysis.
//!
//! The raw JSON stores `nodes` and `edges` as flat integer arrays. We
//! keep them flat (no per-node struct) but:
//!
//! - Convert each edge's `to_node` byte-offset into a real node index,
//!   in place, during build.
//! - Build a CSR-style `first_edge` prefix sum so iterating the edges
//!   of node `i` is `edges[first_edge[i]..first_edge[i+1]]` (each
//!   entry being a single "edge slot" that we then index into the
//!   3-stride raw array).

use anyhow::{Context, Result, anyhow};
use std::path::Path;

use super::raw::{Meta, RawSnapshot};

pub struct HeapGraph {
    pub node_count: usize,
    pub edge_count: usize,
    pub node_stride: usize,
    pub edge_stride: usize,

    pub nodes: Vec<u64>,
    pub edges: Vec<u32>,
    pub strings: Vec<String>,

    pub node_type_names: Vec<String>,
    pub edge_type_names: Vec<String>,

    // Field offsets inside a single node record.
    pub nf_type: usize,
    pub nf_name: usize,
    pub nf_id: usize,
    pub nf_self_size: usize,
    pub nf_edge_count: usize,
    pub nf_detachedness: Option<usize>,

    // Field offsets inside a single edge record.
    pub ef_type: usize,
    pub ef_to_node: usize,

    // Resolved index of the "weak" edge type (if present).
    pub weak_edge_type: Option<u32>,

    // CSR: first_edge[i]..first_edge[i+1] = edge-record indices for node i.
    pub first_edge: Vec<u32>,
}

impl HeapGraph {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let raw: RawSnapshot = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {}", path.display()))?;
        Self::build(raw)
    }

    fn build(raw: RawSnapshot) -> Result<Self> {
        let meta: Meta = raw.snapshot.meta;
        let node_stride = meta.node_fields.len();
        let edge_stride = meta.edge_fields.len();

        if node_stride == 0 || edge_stride == 0 {
            return Err(anyhow!("empty node_fields or edge_fields"));
        }

        let find = |fields: &[String], name: &str| -> Option<usize> {
            fields.iter().position(|f| f == name)
        };

        let nf_type = find(&meta.node_fields, "type")
            .ok_or_else(|| anyhow!("node_fields missing 'type'"))?;
        let nf_name = find(&meta.node_fields, "name")
            .ok_or_else(|| anyhow!("node_fields missing 'name'"))?;
        let nf_id = find(&meta.node_fields, "id")
            .ok_or_else(|| anyhow!("node_fields missing 'id'"))?;
        let nf_self_size = find(&meta.node_fields, "self_size")
            .ok_or_else(|| anyhow!("node_fields missing 'self_size'"))?;
        let nf_edge_count = find(&meta.node_fields, "edge_count")
            .ok_or_else(|| anyhow!("node_fields missing 'edge_count'"))?;
        let nf_detachedness = find(&meta.node_fields, "detachedness");

        let ef_type = find(&meta.edge_fields, "type")
            .ok_or_else(|| anyhow!("edge_fields missing 'type'"))?;
        let ef_to_node = find(&meta.edge_fields, "to_node")
            .ok_or_else(|| anyhow!("edge_fields missing 'to_node'"))?;

        let node_type_names = parse_type_names(&meta.node_types)
            .ok_or_else(|| anyhow!("malformed meta.node_types"))?;
        let edge_type_names = parse_type_names(&meta.edge_types)
            .ok_or_else(|| anyhow!("malformed meta.edge_types"))?;

        let weak_edge_type = edge_type_names
            .iter()
            .position(|t| t == "weak")
            .map(|i| i as u32);

        let nodes = raw.nodes;
        let mut edges = raw.edges;

        if nodes.len() % node_stride != 0 {
            return Err(anyhow!(
                "nodes len {} not a multiple of stride {}",
                nodes.len(),
                node_stride
            ));
        }
        if edges.len() % edge_stride != 0 {
            return Err(anyhow!(
                "edges len {} not a multiple of stride {}",
                edges.len(),
                edge_stride
            ));
        }

        let node_count = nodes.len() / node_stride;
        let edge_count = edges.len() / edge_stride;

        // Convert to_node byte-offset → real node index, in place.
        let stride_u32 = node_stride as u32;
        for i in 0..edge_count {
            let slot = i * edge_stride + ef_to_node;
            let byte_off = edges[slot];
            edges[slot] = byte_off / stride_u32;
        }

        // CSR prefix sum.
        let mut first_edge: Vec<u32> = Vec::with_capacity(node_count + 1);
        let mut running: u32 = 0;
        for i in 0..node_count {
            first_edge.push(running);
            let ec = nodes[i * node_stride + nf_edge_count] as u32;
            running = running
                .checked_add(ec)
                .ok_or_else(|| anyhow!("edge_count overflow at node {}", i))?;
        }
        first_edge.push(running);
        if running as usize != edge_count {
            return Err(anyhow!(
                "edge_count mismatch: CSR sum {}, header {}",
                running,
                edge_count
            ));
        }

        Ok(HeapGraph {
            node_count,
            edge_count,
            node_stride,
            edge_stride,
            nodes,
            edges,
            strings: raw.strings,
            node_type_names,
            edge_type_names,
            nf_type,
            nf_name,
            nf_id,
            nf_self_size,
            nf_edge_count,
            nf_detachedness,
            ef_type,
            ef_to_node,
            weak_edge_type,
            first_edge,
        })
    }

    #[inline]
    pub fn node_type(&self, i: usize) -> u64 {
        self.nodes[i * self.node_stride + self.nf_type]
    }

    pub fn node_type_name(&self, i: usize) -> &str {
        let t = self.node_type(i) as usize;
        self.node_type_names
            .get(t)
            .map(String::as_str)
            .unwrap_or("?")
    }

    #[inline]
    pub fn node_name(&self, i: usize) -> &str {
        let idx = self.nodes[i * self.node_stride + self.nf_name] as usize;
        self.strings.get(idx).map(String::as_str).unwrap_or("")
    }

    #[inline]
    pub fn node_id(&self, i: usize) -> u64 {
        self.nodes[i * self.node_stride + self.nf_id]
    }

    #[inline]
    pub fn node_self_size(&self, i: usize) -> u64 {
        self.nodes[i * self.node_stride + self.nf_self_size]
    }

    #[inline]
    pub fn node_edge_count(&self, i: usize) -> u32 {
        self.nodes[i * self.node_stride + self.nf_edge_count] as u32
    }

    #[inline]
    pub fn node_detached(&self, i: usize) -> bool {
        match self.nf_detachedness {
            Some(off) => self.nodes[i * self.node_stride + off] != 0,
            None => false,
        }
    }

    pub fn edges_of(&self, i: usize) -> EdgesIter<'_> {
        let start = self.first_edge[i] as usize;
        let end = self.first_edge[i + 1] as usize;
        EdgesIter {
            graph: self,
            cur: start,
            end,
        }
    }

    /// Total sum of `self_size` across all nodes.
    pub fn total_self_size(&self) -> u64 {
        let mut total = 0u64;
        for i in 0..self.node_count {
            total = total.saturating_add(self.node_self_size(i));
        }
        total
    }
}

#[derive(Clone, Copy)]
pub struct Edge {
    pub ty: u32,
    pub to: usize,
}

pub struct EdgesIter<'a> {
    graph: &'a HeapGraph,
    cur: usize,
    end: usize,
}

impl<'a> Iterator for EdgesIter<'a> {
    type Item = Edge;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur >= self.end {
            return None;
        }
        let i = self.cur;
        self.cur += 1;
        let base = i * self.graph.edge_stride;
        Some(Edge {
            ty: self.graph.edges[base + self.graph.ef_type],
            to: self.graph.edges[base + self.graph.ef_to_node] as usize,
        })
    }
}

fn parse_type_names(types: &[serde_json::Value]) -> Option<Vec<String>> {
    let first = types.first()?;
    let arr = first.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        out.push(v.as_str()?.to_owned());
    }
    Some(out)
}
