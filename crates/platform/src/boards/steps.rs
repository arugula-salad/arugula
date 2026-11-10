//! Steps (Arugula #762): where each row of a board falls in its order,
//! worked out on every read from the open ordering edges and never stored.
//!
//! A row's step is 1 plus the highest step of the open rows before it. The
//! edges can loop (Plane inserts `blocks` edges with no cycle check), so the
//! rows are first grouped into strongly connected components, iteratively
//! (a board's chain has no depth guard), and the longest path is taken over
//! the components, which can't loop. The rows of a loop share a step and are
//! marked. A blocker off the board, or one the reader can't see, is one step
//! and isn't walked. Doing and Review rows are step 1 whatever their edges
//! say: they started.

use arugula_control_wire::boards as wire;

/// Whether an open `kind` edge puts its first end before its second in a
/// board's steps. The one place that says which kinds order.
pub fn orders_steps(kind: wire::RelationKind) -> bool {
    matches!(kind, wire::RelationKind::Blocks | wire::RelationKind::Before)
}

/// One row, as the steps see it.
#[derive(Default, Clone)]
pub struct Node {
    /// An open row with an issue or a PR: it has a step.
    pub open: bool,
    /// In Doing or Review: step 1.
    pub started: bool,
    /// The rows (by index) with an open ordering edge or stacked PR before
    /// this one.
    pub before: Vec<usize>,
    /// An open ordering blocker off the board, or one the reader can't see.
    pub off: bool,
}

/// What [`work_out`] found, by row index.
pub struct Steps {
    pub step: Vec<Option<u32>>,
    pub cycle: Vec<bool>,
    /// The rows with a step: by step, then by what they unblock (most
    /// first), then by index.
    pub sequence: Vec<usize>,
    /// The longest chain, first to last; empty when it has one row.
    pub chain: Vec<usize>,
}

pub fn work_out(nodes: &[Node]) -> Steps {
    let n = nodes.len();
    // `next[j]` holds the rows that come after `j`. An edge into a started
    // row is dropped: it says nothing of a row that began.
    let mut next: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, node) in nodes.iter().enumerate().filter(|(_, x)| x.open && !x.started) {
        for &j in node.before.iter().filter(|&&j| nodes[j].open) {
            next[j].push(i);
        }
    }
    let (comp, comps) = components(&next);
    let looped: Vec<bool> =
        (0..n).map(|i| nodes[i].open && (comps[comp[i]].len() > 1 || next[i].contains(&i))).collect();

    // Tarjan finds a component only after everything it points at, so the
    // reverse of that order has every component after those before it.
    let mut level = vec![0u32; comps.len()];
    let mut via: Vec<Option<usize>> = vec![None; comps.len()];
    for c in (0..comps.len()).rev() {
        let first = comps[c][0];
        if !nodes[first].open {
            continue;
        }
        let off = comps[c].iter().any(|&i| nodes[i].off && !nodes[i].started);
        level[c] = level[c].max(if off { 2 } else { 1 });
        for &i in &comps[c] {
            for &k in &next[i] {
                let d = comp[k];
                if d != c && level[d] < level[c] + 1 {
                    level[d] = level[c] + 1;
                    via[d] = Some(c);
                }
            }
        }
    }
    let step: Vec<Option<u32>> = (0..n).map(|i| nodes[i].open.then(|| level[comp[i]])).collect();

    let unblocks: Vec<usize> = (0..n).map(|i| reach(&next, i)).collect();
    let mut sequence: Vec<usize> = (0..n).filter(|&i| step[i].is_some()).collect();
    sequence.sort_by_key(|&i| (step[i], std::cmp::Reverse(unblocks[i]), i));

    // The chain ends at the first row of the deepest step and walks back
    // through the components that raised each level; a component is its
    // first row.
    let mut chain = vec![];
    let deepest = step.iter().flatten().max().copied();
    if let Some(end) = (0..n).find(|&i| step[i].is_some() && step[i] == deepest) {
        let mut c = Some(comp[end]);
        while let Some(at) = c {
            chain.push(comps[at][0]);
            c = via[at];
        }
        chain.reverse();
    }
    if chain.len() < 2 {
        chain.clear();
    }
    Steps { step, cycle: looped, sequence, chain }
}

/// The strongly connected components of the graph `next`, iteratively (Tarjan):
/// each row's component, and the components in the order they were found.
fn components(next: &[Vec<usize>]) -> (Vec<usize>, Vec<Vec<usize>>) {
    const NONE: usize = usize::MAX;
    let n = next.len();
    let (mut index, mut low) = (vec![NONE; n], vec![0usize; n]);
    let (mut on, mut comp) = (vec![false; n], vec![NONE; n]);
    let (mut stack, mut comps, mut counter) = (vec![], vec![], 0);
    for root in 0..n {
        if index[root] != NONE {
            continue;
        }
        // A frame is a row and how far into its edges we are.
        let mut work = vec![(root, 0usize)];
        while let Some(&(v, at)) = work.last() {
            if at == 0 {
                index[v] = counter;
                low[v] = counter;
                counter += 1;
                stack.push(v);
                on[v] = true;
            }
            if let Some(&w) = next[v].get(at) {
                work.last_mut().expect("a frame").1 += 1;
                if index[w] == NONE {
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut members = vec![];
                while let Some(w) = stack.pop() {
                    on[w] = false;
                    comp[w] = comps.len();
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                members.sort_unstable();
                comps.push(members);
            }
        }
    }
    (comp, comps)
}

/// How many rows are after `from`, however far.
fn reach(next: &[Vec<usize>], from: usize) -> usize {
    let mut seen = vec![false; next.len()];
    seen[from] = true;
    let mut todo = vec![from];
    let mut count = 0;
    while let Some(v) = todo.pop() {
        for &w in &next[v] {
            if !seen[w] {
                seen[w] = true;
                count += 1;
                todo.push(w);
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(before: &[usize]) -> Node {
        Node { open: true, before: before.to_vec(), ..Default::default() }
    }

    /// A row is one step past the deepest open row before it, a done or
    /// item-less row has none, and the longest chain is named.
    #[test]
    fn a_row_is_one_step_past_the_deepest_before_it() {
        let done = Node::default();
        let nodes = [open(&[]), open(&[0]), open(&[0, 1]), open(&[]), done, open(&[4])];
        let s = work_out(&nodes);
        assert_eq!(s.step, [Some(1), Some(2), Some(3), Some(1), None, Some(1)]);
        assert_eq!(s.chain, [0, 1, 2]);
        assert!(s.cycle.iter().all(|c| !c));
    }

    /// A loop shares a step and is marked; what waits on it comes after.
    #[test]
    fn a_loop_shares_a_step_and_what_waits_on_it_comes_after() {
        let nodes = [open(&[1]), open(&[0]), open(&[1]), open(&[2]), open(&[4])];
        let s = work_out(&nodes);
        assert_eq!(s.step, [Some(1), Some(1), Some(2), Some(3), Some(1)]);
        assert_eq!(s.cycle, [true, true, false, false, true], "a row that waits on itself is a loop");
        assert_eq!(s.chain, [0, 2, 3]);
    }

    /// Started rows are step 1 whatever is before them; a blocker off the
    /// board is one step.
    #[test]
    fn a_started_row_is_step_one_and_an_off_board_blocker_is_one_step() {
        let started = Node { started: true, ..open(&[1]) };
        let off = Node { off: true, ..open(&[]) };
        let off_started = Node { off: true, started: true, ..open(&[]) };
        let s = work_out(&[started, open(&[]), off, open(&[2]), off_started]);
        assert_eq!(s.step, [Some(1), Some(1), Some(2), Some(3), Some(1)]);
        assert!(s.cycle.iter().all(|c| !c));
    }

    /// Inside a step, what unblocks the most comes first, then the order the
    /// rows are in.
    #[test]
    fn inside_a_step_what_unblocks_the_most_comes_first() {
        // 0 and 1 and 2 are step 1; 2 unblocks two rows, 1 one, 0 none.
        let nodes = [open(&[]), open(&[]), open(&[]), open(&[1]), open(&[2]), open(&[4])];
        assert_eq!(work_out(&nodes).sequence[..3], [2, 1, 0]);
    }

    /// A place is the steps' layout over every row with an item, with the
    /// edges of done rows kept (#824): A(done) -> B(done) -> C -> D sit in
    /// 1, 2, 3, 4, though the steps of the same board are C 1 and D 2, and a
    /// place that dropped a done row's edges would put C and D in 1 and 2.
    #[test]
    fn a_place_keeps_the_edges_of_done_rows() {
        let (a, b) = (Node::default(), Node { before: vec![0], ..Default::default() });
        // `step`: A and B are done, and an edge from a closed row is dropped.
        let steps = work_out(&[a, b, open(&[]), open(&[2])]);
        assert_eq!(steps.step, [None, None, Some(1), Some(2)]);
        // `place`: every row with an item is open, and every edge is kept.
        let place = work_out(&[open(&[]), open(&[0]), open(&[1]), open(&[2])]);
        assert_eq!(place.step, [Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(place.chain, [0, 1, 2, 3]);
    }

    /// A long chain is no deeper than the stack: nothing recurses.
    #[test]
    fn a_long_chain_does_not_recurse() {
        let nodes: Vec<Node> = (0..5_000).map(|i| open(&if i == 0 { vec![] } else { vec![i - 1] })).collect();
        let s = work_out(&nodes);
        assert_eq!(s.step[4_999], Some(5_000));
        assert_eq!(s.chain.len(), 5_000);
    }
}
