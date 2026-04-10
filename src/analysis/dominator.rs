//! Iterative dominator tree + retained-size computation.
//!
//! Uses the Cooper-Harvey-Kennedy algorithm ("A Simple, Fast Dominance
//! Algorithm", 2001). It is O(n·m) worst case but O(n) in practice on
//! tree-like graphs, which is what heap snapshots almost always are.
//!
//! Inputs: a `HeapGraph` whose node 0 is the V8 synthetic GC root.
//! Outputs: for each reachable node, an immediate dominator and a
//! retained size. Unreachable nodes get `idom = -1` and
//! `retained_size == self_size`.

use crate::parser::HeapGraph;

pub struct Dominators {
    /// Immediate dominator per node (as node index), or -1 if unreachable.
    pub idom: Vec<i32>,
    /// Retained size per node (= self_size for unreachable nodes).
    pub retained_size: Vec<u64>,
    pub unreachable_count: usize,
    pub unreachable_self_size: u64,
}

pub fn compute(graph: &HeapGraph) -> Dominators {
    let n = graph.node_count;
    let root: usize = 0;
    let weak = graph.weak_edge_type;

    let (post_order, order_list) = dfs_post_order(graph, root, weak, n);
    let (pred_first, pred_list) = build_predecessors(graph, &post_order, weak, n);

    let mut idom: Vec<i32> = vec![-1; n];
    if n > 0 {
        idom[root] = root as i32;
    }

    // CHK main loop. Iterate in reverse post-order (root first).
    let mut changed = true;
    let mut guard = 0u32;
    while changed {
        changed = false;
        guard += 1;
        if guard > 64 {
            // Safety valve. CHK converges in a small number of passes
            // on real programs; if we're past 64 something is very wrong.
            break;
        }
        for &node in order_list.iter().rev() {
            if node == root {
                continue;
            }
            let preds_start = pred_first[node] as usize;
            let preds_end = pred_first[node + 1] as usize;

            let mut new_idom: i32 = -1;
            for i in preds_start..preds_end {
                let p = pred_list[i] as usize;
                if idom[p] != -1 {
                    new_idom = p as i32;
                    break;
                }
            }
            if new_idom < 0 {
                continue;
            }

            for i in preds_start..preds_end {
                let p = pred_list[i] as i32;
                if p == new_idom {
                    continue;
                }
                if idom[p as usize] != -1 {
                    new_idom = intersect(p, new_idom, &post_order, &idom);
                }
            }

            if idom[node] != new_idom {
                idom[node] = new_idom;
                changed = true;
            }
        }
    }

    // Retained sizes: init with self_size, then bottom-up sum in dom tree.
    let mut retained_size: Vec<u64> = Vec::with_capacity(n);
    for i in 0..n {
        retained_size.push(graph.node_self_size(i));
    }
    for &node in &order_list {
        if node == root {
            continue;
        }
        let dom = idom[node];
        if dom < 0 {
            continue;
        }
        let add = retained_size[node];
        let slot = &mut retained_size[dom as usize];
        *slot = slot.saturating_add(add);
    }

    let unreachable_count = n - order_list.len();
    let mut unreachable_self_size = 0u64;
    for i in 0..n {
        if post_order[i] < 0 {
            unreachable_self_size =
                unreachable_self_size.saturating_add(graph.node_self_size(i));
        }
    }

    Dominators {
        idom,
        retained_size,
        unreachable_count,
        unreachable_self_size,
    }
}

fn intersect(b1: i32, b2: i32, post_order: &[i32], idom: &[i32]) -> i32 {
    let mut f1 = b1;
    let mut f2 = b2;
    while f1 != f2 {
        while post_order[f1 as usize] < post_order[f2 as usize] {
            f1 = idom[f1 as usize];
            if f1 < 0 {
                return f2;
            }
        }
        while post_order[f2 as usize] < post_order[f1 as usize] {
            f2 = idom[f2 as usize];
            if f2 < 0 {
                return f1;
            }
        }
    }
    f1
}

/// Iterative DFS — heap graphs can be millions of nodes deep, so a
/// recursive implementation would stack-overflow.
fn dfs_post_order(
    graph: &HeapGraph,
    root: usize,
    weak: Option<u32>,
    n: usize,
) -> (Vec<i32>, Vec<usize>) {
    let mut post_order = vec![-1i32; n];
    let mut order_list: Vec<usize> = Vec::with_capacity(n);

    if n == 0 || root >= n {
        return (post_order, order_list);
    }

    let mut visited = vec![false; n];
    visited[root] = true;

    // (node, next edge slot to try, end of edge slot range)
    let mut stack: Vec<(usize, u32, u32)> =
        Vec::with_capacity(1024);
    stack.push((
        root,
        graph.first_edge[root],
        graph.first_edge[root + 1],
    ));

    while !stack.is_empty() {
        let top = stack.len() - 1;
        let (node, mut cur, end) = stack[top];
        let mut descended = false;

        while cur < end {
            let base = (cur as usize) * graph.edge_stride;
            cur += 1;
            let ty = graph.edges[base + graph.ef_type];
            if Some(ty) == weak {
                continue;
            }
            let to = graph.edges[base + graph.ef_to_node] as usize;
            if !visited[to] {
                visited[to] = true;
                stack[top].1 = cur;
                stack.push((
                    to,
                    graph.first_edge[to],
                    graph.first_edge[to + 1],
                ));
                descended = true;
                break;
            }
        }

        if !descended {
            stack.pop();
            post_order[node] = order_list.len() as i32;
            order_list.push(node);
        }
    }

    (post_order, order_list)
}

/// CSR reverse adjacency for reachable nodes, skipping weak edges.
fn build_predecessors(
    graph: &HeapGraph,
    post_order: &[i32],
    weak: Option<u32>,
    n: usize,
) -> (Vec<u32>, Vec<u32>) {
    let mut in_deg: Vec<u32> = vec![0; n];
    for i in 0..n {
        if post_order[i] < 0 {
            continue;
        }
        for e in graph.edges_of(i) {
            if Some(e.ty) == weak {
                continue;
            }
            if post_order[e.to] < 0 {
                continue;
            }
            in_deg[e.to] += 1;
        }
    }

    let mut first: Vec<u32> = Vec::with_capacity(n + 1);
    let mut running: u32 = 0;
    for &d in &in_deg {
        first.push(running);
        running = running.saturating_add(d);
    }
    first.push(running);

    let mut list: Vec<u32> = vec![0u32; running as usize];
    let mut next: Vec<u32> = first.clone();
    for i in 0..n {
        if post_order[i] < 0 {
            continue;
        }
        for e in graph.edges_of(i) {
            if Some(e.ty) == weak {
                continue;
            }
            if post_order[e.to] < 0 {
                continue;
            }
            let slot = next[e.to] as usize;
            list[slot] = i as u32;
            next[e.to] += 1;
        }
    }

    (first, list)
}
