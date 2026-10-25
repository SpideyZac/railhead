//! Grid pathfinding (A*) shared by building tools, trucks, raiders and crews.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::types::{Dir, Pos};

/// A* on a 4-connected grid. `cost(from, to)` returns None when the step is
/// impossible. Costs are integers (scale them up for fractional weights).
/// `min_step` is the cheapest possible step cost, used for the heuristic.
pub fn astar(
    w: i32,
    h: i32,
    start: Pos,
    goal: impl Fn(Pos) -> bool,
    goal_hint: Pos,
    min_step: u32,
    mut cost: impl FnMut(Pos, Pos) -> Option<u32>,
) -> Option<Vec<Pos>> {
    let idx = |p: Pos| (p.y * w + p.x) as usize;
    let n = (w * h) as usize;
    let mut best = vec![u32::MAX; n];
    let mut prev = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    best[idx(start)] = 0;
    heap.push(Reverse((0u32, 0u32, start.x, start.y)));
    while let Some(Reverse((_, g, x, y))) = heap.pop() {
        let p = Pos::new(x, y);
        if g > best[idx(p)] {
            continue;
        }
        if goal(p) {
            let mut path = vec![p];
            let mut cur = idx(p);
            while prev[cur] != u32::MAX {
                cur = prev[cur] as usize;
                path.push(Pos::new(cur as i32 % w, cur as i32 / w));
            }
            path.reverse();
            return Some(path);
        }
        for d in Dir::ALL {
            let q = p.step(d);
            if q.x < 0 || q.y < 0 || q.x >= w || q.y >= h {
                continue;
            }
            let Some(c) = cost(p, q) else { continue };
            let ng = g + c;
            if ng < best[idx(q)] {
                best[idx(q)] = ng;
                prev[idx(q)] = idx(p) as u32;
                let hcost = q.manhattan(goal_hint) as u32 * min_step;
                heap.push(Reverse((ng + hcost, ng, q.x, q.y)));
            }
        }
    }
    None
}

/// A* over (tile, heading), so a bend can cost extra: lines come out
/// straight instead of staircased. `turn` is added whenever the direction
/// changes (not on the first step). `min_step` is the cheapest possible step.
pub fn astar_turns(
    w: i32,
    h: i32,
    start: Pos,
    goal: Pos,
    min_step: u32,
    turn: u32,
    mut cost: impl FnMut(Pos, Pos) -> Option<u32>,
) -> Option<Vec<Pos>> {
    // state = tile * 5 + heading (0..4 = Dir::ALL, 4 = not moved yet)
    let n = (w * h) as usize * 5;
    let st = |p: Pos, d: usize| (p.y * w + p.x) as usize * 5 + d;
    let mut best = vec![u32::MAX; n];
    let mut prev = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    best[st(start, 4)] = 0;
    heap.push(Reverse((0u32, 0u32, start.x, start.y, 4usize)));
    while let Some(Reverse((_, g, x, y, d))) = heap.pop() {
        let p = Pos::new(x, y);
        let s = st(p, d);
        if g > best[s] {
            continue;
        }
        if p == goal {
            let mut path = vec![p];
            let mut cur = s;
            while prev[cur] != u32::MAX {
                cur = prev[cur] as usize;
                let t = (cur / 5) as i32;
                path.push(Pos::new(t % w, t / w));
            }
            path.reverse();
            return Some(path);
        }
        for (nd, dir) in Dir::ALL.iter().enumerate() {
            let q = p.step(*dir);
            if q.x < 0 || q.y < 0 || q.x >= w || q.y >= h {
                continue;
            }
            let Some(c) = cost(p, q) else { continue };
            let ng = g + c + if d != 4 && d != nd { turn } else { 0 };
            let ns = st(q, nd);
            if ng < best[ns] {
                best[ns] = ng;
                prev[ns] = s as u32;
                let hcost = q.manhattan(goal) as u32 * min_step;
                heap.push(Reverse((ng + hcost, ng, q.x, q.y, nd)));
            }
        }
    }
    None
}

/// Breadth-first flood fill; returns a visited mask.
pub fn flood(w: i32, h: i32, start: Pos, passable: impl Fn(Pos) -> bool) -> Vec<bool> {
    let mut seen = vec![false; (w * h) as usize];
    let mut q = std::collections::VecDeque::new();
    if !passable(start) {
        return seen;
    }
    seen[(start.y * w + start.x) as usize] = true;
    q.push_back(start);
    while let Some(p) = q.pop_front() {
        for d in Dir::ALL {
            let n = p.step(d);
            if n.x < 0 || n.y < 0 || n.x >= w || n.y >= h {
                continue;
            }
            let i = (n.y * w + n.x) as usize;
            if !seen[i] && passable(n) {
                seen[i] = true;
                q.push_back(n);
            }
        }
    }
    seen
}
