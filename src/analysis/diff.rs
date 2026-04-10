use std::collections::{BTreeSet, HashMap};

use crate::analysis::dominator::Dominators;
use crate::parser::HeapGraph;

pub struct Diff {
    pub a_name: String,
    pub b_name: String,
    pub a_self: u64,
    pub b_self: u64,
    pub a_retained: u64,
    pub b_retained: u64,
    pub a_nodes: usize,
    pub b_nodes: usize,
    /// Type deltas sorted by abs(Δ size) desc, limited to N entries.
    pub type_deltas: Vec<TypeDelta>,
    /// New nodes in B (not in A), top-N by self_size.
    pub new_nodes: Vec<DiffNodeRow>,
    /// Gone nodes (in A but not in B), top-N by self_size.
    pub gone_nodes: Vec<DiffNodeRow>,
}

pub struct TypeDelta {
    pub name: String,
    pub count_a: u64,
    pub count_b: u64,
    pub size_a: u64,
    pub size_b: u64,
}

pub struct DiffNodeRow {
    pub type_name: String,
    pub name: String,
    pub self_size: u64,
    pub retained_size: u64,
    pub id: u64,
}

pub fn compute(
    a: &HeapGraph,
    a_dom: &Dominators,
    a_name: String,
    b: &HeapGraph,
    b_dom: &Dominators,
    b_name: String,
    limit: usize,
) -> Diff {
    let mut a_ids: HashMap<u64, usize> = HashMap::with_capacity(a.node_count);
    for i in 0..a.node_count {
        a_ids.insert(a.node_id(i), i);
    }
    let mut b_ids: HashMap<u64, usize> = HashMap::with_capacity(b.node_count);
    for i in 0..b.node_count {
        b_ids.insert(b.node_id(i), i);
    }

    // Type histograms keyed by type name.
    let mut a_hist: HashMap<String, (u64, u64)> = HashMap::new();
    for i in 0..a.node_count {
        let e = a_hist
            .entry(a.node_type_name(i).to_owned())
            .or_insert((0, 0));
        e.0 += 1;
        e.1 = e.1.saturating_add(a.node_self_size(i));
    }
    let mut b_hist: HashMap<String, (u64, u64)> = HashMap::new();
    for i in 0..b.node_count {
        let e = b_hist
            .entry(b.node_type_name(i).to_owned())
            .or_insert((0, 0));
        e.0 += 1;
        e.1 = e.1.saturating_add(b.node_self_size(i));
    }

    let all_types: BTreeSet<String> = a_hist.keys().chain(b_hist.keys()).cloned().collect();
    let mut type_deltas: Vec<TypeDelta> = all_types
        .into_iter()
        .map(|name| {
            let (ca, sa) = a_hist.get(&name).copied().unwrap_or((0, 0));
            let (cb, sb) = b_hist.get(&name).copied().unwrap_or((0, 0));
            TypeDelta {
                name,
                count_a: ca,
                count_b: cb,
                size_a: sa,
                size_b: sb,
            }
        })
        .filter(|td| td.size_a != td.size_b || td.count_a != td.count_b)
        .collect();

    type_deltas.sort_by(|x, y| {
        let dx = (x.size_b as i128 - x.size_a as i128).unsigned_abs();
        let dy = (y.size_b as i128 - y.size_a as i128).unsigned_abs();
        dy.cmp(&dx)
    });

    // New & gone nodes by id.
    let mut new_nodes: Vec<usize> = (0..b.node_count)
        .filter(|&i| !a_ids.contains_key(&b.node_id(i)))
        .collect();
    new_nodes.sort_unstable_by(|&x, &y| b.node_self_size(y).cmp(&b.node_self_size(x)));
    new_nodes.truncate(limit);

    let mut gone_nodes: Vec<usize> = (0..a.node_count)
        .filter(|&i| !b_ids.contains_key(&a.node_id(i)))
        .collect();
    gone_nodes.sort_unstable_by(|&x, &y| a.node_self_size(y).cmp(&a.node_self_size(x)));
    gone_nodes.truncate(limit);

    let new_rows: Vec<DiffNodeRow> = new_nodes
        .into_iter()
        .map(|i| DiffNodeRow {
            type_name: b.node_type_name(i).to_owned(),
            name: b.node_name(i).to_owned(),
            self_size: b.node_self_size(i),
            retained_size: b_dom.retained_size[i],
            id: b.node_id(i),
        })
        .collect();

    let gone_rows: Vec<DiffNodeRow> = gone_nodes
        .into_iter()
        .map(|i| DiffNodeRow {
            type_name: a.node_type_name(i).to_owned(),
            name: a.node_name(i).to_owned(),
            self_size: a.node_self_size(i),
            retained_size: a_dom.retained_size[i],
            id: a.node_id(i),
        })
        .collect();

    Diff {
        a_name,
        b_name,
        a_self: a.total_self_size(),
        b_self: b.total_self_size(),
        a_retained: if a.node_count > 0 {
            a_dom.retained_size[0]
        } else {
            0
        },
        b_retained: if b.node_count > 0 {
            b_dom.retained_size[0]
        } else {
            0
        },
        a_nodes: a.node_count,
        b_nodes: b.node_count,
        type_deltas,
        new_nodes: new_rows,
        gone_nodes: gone_rows,
    }
}
