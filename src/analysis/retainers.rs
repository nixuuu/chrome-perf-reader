use crate::analysis::dominator::Dominators;
use crate::parser::HeapGraph;

pub struct TopRetainers {
    pub by_self: Vec<Row>,
    pub by_retained: Vec<Row>,
}

pub struct Row {
    pub rank: usize,
    pub type_name: String,
    pub name: String,
    pub self_size: u64,
    pub retained_size: u64,
    pub edge_count: u32,
    pub id: u64,
}

pub fn compute(graph: &HeapGraph, dom: &Dominators, limit: usize) -> TopRetainers {
    let n = graph.node_count;

    let mut by_self: Vec<(usize, u64)> = (0..n)
        .map(|i| (i, graph.node_self_size(i)))
        .collect();
    by_self.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    by_self.truncate(limit);

    // Synthetic root (node 0) always dominates everything, so exclude it.
    let mut by_ret: Vec<(usize, u64)> = (0..n)
        .filter(|&i| i != 0)
        .map(|i| (i, dom.retained_size[i]))
        .collect();
    by_ret.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    by_ret.truncate(limit);

    let make_row = |rank: usize, (idx, _): (usize, u64)| -> Row {
        Row {
            rank,
            type_name: graph.node_type_name(idx).to_owned(),
            name: graph.node_name(idx).to_owned(),
            self_size: graph.node_self_size(idx),
            retained_size: dom.retained_size[idx],
            edge_count: graph.node_edge_count(idx),
            id: graph.node_id(idx),
        }
    };

    TopRetainers {
        by_self: by_self
            .into_iter()
            .enumerate()
            .map(|(r, p)| make_row(r + 1, p))
            .collect(),
        by_retained: by_ret
            .into_iter()
            .enumerate()
            .map(|(r, p)| make_row(r + 1, p))
            .collect(),
    }
}
