//! Graph algorithms for the compiler: reachability (FR-GRAPH-09) and feedback loops (Tarjan's strongly connected components).

use super::*;

/// The right copy's slot of the module in `slot`, if it runs as two.
/// `copies` is [`Schedule::copies`].
pub(crate) fn partner(copies: &[(u32, u32)], slot: usize) -> Option<usize> {
    let slot = u32::try_from(slot).ok()?;
    let index = copies.binary_search_by_key(&slot, |&(left, _)| left).ok()?;
    Some(copies[index].1 as usize)
}

/// True for each module that the user can hear or see, or that feeds one
/// over cables and assignments (FR-GRAPH-09). `succ` lists each module's
/// successors. The roots are:
///
/// - the audio output, which the user hears;
/// - a module that `shown` names: one with a probed output, whose meter or
///   scope the user watches (`docs/design/telemetry.md`), or one with a
///   modulated parameter, whose knob shows a moving live dot;
/// - a module that publishes display values or has a light or meter on its
///   panel (`docs/design/panels.md`). It keeps running
///   unconnected, so its lights tell the truth. MIDI Note is one, so it
///   follows every note on and off even with no cable, and a cable added
///   later never finds a stuck note.
///
/// Walks backwards from the roots, so the cost is linear in modules and
/// edges.
pub(super) fn reaches_a_root(
    nodes: &[&crate::graph::Node],
    succ: &[Vec<usize>],
    shown: impl Fn(usize) -> bool,
) -> Vec<bool> {
    let n = nodes.len();
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, next) in succ.iter().enumerate() {
        for &v in next {
            pred[v].push(u);
        }
    }
    let is_root = |v: usize| {
        let descriptor = nodes[v].descriptor;
        matches!(descriptor.role, ModuleRole::AudioOutput)
            || shown(v)
            || !descriptor.displays.is_empty()
            || descriptor
                .panel
                .widgets
                .iter()
                .any(|w| matches!(w.kind, crate::WidgetKind::Light | crate::WidgetKind::Meter))
    };
    let mut live = vec![false; n];
    let mut stack: Vec<usize> = (0..n).filter(|&v| is_root(v)).collect();
    for &v in &stack {
        live[v] = true;
    }
    while let Some(v) = stack.pop() {
        for &u in &pred[v] {
            if !live[u] {
                live[u] = true;
                stack.push(u);
            }
        }
    }
    live
}

/// Tarjan's strongly connected components. Returns SCCs in reverse
/// topological order, each sorted.
pub(super) fn tarjan(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State<'a> {
        succ: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }

    impl State<'_> {
        fn visit(&mut self, v: usize) {
            self.index[v] = Some(self.next);
            self.low[v] = self.next;
            self.next += 1;
            self.stack.push(v);
            self.on_stack[v] = true;
            for i in 0..self.succ[v].len() {
                let w = self.succ[v][i];
                match self.index[w] {
                    None => {
                        self.visit(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(wi) if self.on_stack[w] => self.low[v] = self.low[v].min(wi),
                    Some(_) => {}
                }
            }
            if Some(self.low[v]) == self.index[v] {
                let mut component = Vec::new();
                loop {
                    let w = self.stack.pop().expect("tarjan stack");
                    self.on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                self.out.push(component);
            }
        }
    }

    let n = succ.len();
    let mut state = State {
        succ,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if state.index[v].is_none() {
            state.visit(v);
        }
    }
    state.out
}
