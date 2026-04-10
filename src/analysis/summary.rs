use crate::analysis::dominator::Dominators;
use crate::parser::HeapGraph;

pub struct Summary {
    pub file_name: String,
    pub file_size: u64,
    pub node_count: usize,
    pub edge_count: usize,
    pub string_count: usize,
    pub total_self_size: u64,
    pub total_retained_from_root: u64,
    pub unreachable_count: usize,
    pub unreachable_self_size: u64,
    /// Sorted by total_self_size desc.
    pub node_type_histogram: Vec<TypeBucket>,
    /// Sorted by count desc.
    pub edge_type_histogram: Vec<TypeBucket>,
}

pub struct TypeBucket {
    pub name: String,
    pub count: u64,
    pub total_self_size: u64,
}

pub fn compute(
    graph: &HeapGraph,
    dom: &Dominators,
    file_name: String,
    file_size: u64,
) -> Summary {
    let total_self_size = graph.total_self_size();
    let total_retained_from_root = if graph.node_count > 0 {
        dom.retained_size[0]
    } else {
        0
    };

    // Node type histogram.
    let type_count = graph.node_type_names.len();
    let mut n_hist: Vec<(u64, u64)> = vec![(0, 0); type_count];
    for i in 0..graph.node_count {
        let t = graph.node_type(i) as usize;
        if t < type_count {
            n_hist[t].0 += 1;
            n_hist[t].1 = n_hist[t].1.saturating_add(graph.node_self_size(i));
        }
    }
    let mut node_type_histogram: Vec<TypeBucket> = n_hist
        .into_iter()
        .enumerate()
        .map(|(i, (c, s))| TypeBucket {
            name: graph.node_type_names[i].clone(),
            count: c,
            total_self_size: s,
        })
        .filter(|b| b.count > 0)
        .collect();
    node_type_histogram.sort_by(|a, b| b.total_self_size.cmp(&a.total_self_size));

    // Edge type histogram (count only; edges have no size).
    let etype_count = graph.edge_type_names.len();
    let mut e_hist: Vec<u64> = vec![0; etype_count];
    for i in 0..graph.edge_count {
        let ty = graph.edges[i * graph.edge_stride + graph.ef_type] as usize;
        if ty < etype_count {
            e_hist[ty] += 1;
        }
    }
    let mut edge_type_histogram: Vec<TypeBucket> = e_hist
        .into_iter()
        .enumerate()
        .map(|(i, c)| TypeBucket {
            name: graph.edge_type_names[i].clone(),
            count: c,
            total_self_size: 0,
        })
        .filter(|b| b.count > 0)
        .collect();
    edge_type_histogram.sort_by(|a, b| b.count.cmp(&a.count));

    Summary {
        file_name,
        file_size,
        node_count: graph.node_count,
        edge_count: graph.edge_count,
        string_count: graph.strings.len(),
        total_self_size,
        total_retained_from_root,
        unreachable_count: dom.unreachable_count,
        unreachable_self_size: dom.unreachable_self_size,
        node_type_histogram,
        edge_type_histogram,
    }
}
