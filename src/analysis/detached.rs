use crate::analysis::dominator::Dominators;
use crate::parser::HeapGraph;

pub struct Detached {
    pub total_count: usize,
    pub total_retained: u64,
    pub top: Vec<DetachedRow>,
}

pub struct DetachedRow {
    pub type_name: String,
    pub name: String,
    pub self_size: u64,
    pub retained_size: u64,
    pub id: u64,
    /// Chain of immediate dominators walking toward the root.
    /// First entry is the node's direct dominator, last is closest to root.
    pub dominator_chain: Vec<DomStep>,
}

pub struct DomStep {
    pub type_name: String,
    pub name: String,
}

const CHAIN_DEPTH: usize = 6;

pub fn compute(graph: &HeapGraph, dom: &Dominators, limit: usize) -> Detached {
    if graph.nf_detachedness.is_none() {
        return Detached {
            total_count: 0,
            total_retained: 0,
            top: Vec::new(),
        };
    }

    let mut detached: Vec<(usize, u64)> = Vec::new();
    for i in 0..graph.node_count {
        if graph.node_detached(i) {
            detached.push((i, dom.retained_size[i]));
        }
    }
    let total_count = detached.len();
    let total_retained: u64 = detached
        .iter()
        .map(|(_, s)| *s)
        .fold(0u64, |a, b| a.saturating_add(b));

    detached.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    detached.truncate(limit);

    let top: Vec<DetachedRow> = detached
        .into_iter()
        .map(|(idx, _)| {
            let mut chain: Vec<DomStep> = Vec::new();
            let mut cur = idx;
            for _ in 0..CHAIN_DEPTH {
                let d = dom.idom[cur];
                if d < 0 {
                    break;
                }
                let d = d as usize;
                if d == cur {
                    break;
                }
                chain.push(DomStep {
                    type_name: graph.node_type_name(d).to_owned(),
                    name: graph.node_name(d).to_owned(),
                });
                cur = d;
                if cur == 0 {
                    break;
                }
            }
            DetachedRow {
                type_name: graph.node_type_name(idx).to_owned(),
                name: graph.node_name(idx).to_owned(),
                self_size: graph.node_self_size(idx),
                retained_size: dom.retained_size[idx],
                id: graph.node_id(idx),
                dominator_chain: chain,
            }
        })
        .collect();

    Detached {
        total_count,
        total_retained,
        top,
    }
}
