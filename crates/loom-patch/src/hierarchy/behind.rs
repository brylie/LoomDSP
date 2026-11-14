//! Following ends through a level, with what was found kept for one pass,
//! such as one scene build. A chain of empty thru slots or pass-through
//! ports is walked once, not once per end on it, so a long chain costs
//! work in proportion to its length, not its square (sub-patches.md,
//! Empty slots).

use super::{End, Hierarchy};
use crate::flatten::ModulePath;
use loom_core::{Graph, ModuleId};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// An end and the level it belongs to.
type Key = (ModulePath, End);

/// A flat module and one of its ports, by name.
type Flat = (ModuleId, String);

/// One step back from an output end.
pub(crate) enum Step {
    /// The end is this flat output.
    Found(Flat),
    /// The end stands for the end one link further back.
    Next(ModulePath, End),
}

/// One step on from an input end.
pub(crate) enum InputStep {
    /// The end is this flat input.
    Found(Flat),
    /// The end feeds these ends, in order.
    Next(Vec<Key>),
    /// Nothing stands behind the end.
    Nothing,
}

/// The follows of one level, kept for one pass. Make one for a pass over
/// a patch that does not change, and drop it after: a kept answer is wrong
/// once a link or a slot changes.
pub struct Behind<'h> {
    hierarchy: &'h Hierarchy,
    graph: &'h Graph,
    sources: RefCell<BTreeMap<Key, Option<Flat>>>,
    firsts: RefCell<BTreeMap<Key, Option<Flat>>>,
}

impl<'h> Behind<'h> {
    /// An empty memo over `hierarchy` and its flat `graph`.
    pub fn new(hierarchy: &'h Hierarchy, graph: &'h Graph) -> Self {
        Self {
            hierarchy,
            graph,
            sources: RefCell::new(BTreeMap::new()),
            firsts: RefCell::new(BTreeMap::new()),
        }
    }

    /// What [`Hierarchy::source_behind`] gives. Every end met on the way
    /// keeps the answer: each end has one step back, so the chain's answer
    /// is the answer from any end on it, and an end in a loop has none.
    pub fn source(&self, level: &ModulePath, end: &End) -> Option<Flat> {
        let mut sources = self.sources.borrow_mut();
        let mut walked: Vec<Key> = Vec::new();
        let mut on_walk: BTreeSet<Key> = BTreeSet::new();
        let mut at: Key = (level.clone(), end.clone());
        let answer = loop {
            if let Some(known) = sources.get(&at) {
                break known.clone();
            }
            if !on_walk.insert(at.clone()) {
                break None;
            }
            walked.push(at.clone());
            match self.hierarchy.source_step(self.graph, &at.0, &at.1) {
                None => break None,
                Some(Step::Found(flat)) => break Some(flat),
                Some(Step::Next(next_level, next_end)) => at = (next_level, next_end),
            }
        };
        for key in walked {
            sources.insert(key, answer.clone());
        }
        answer
    }

    /// The first of what [`Hierarchy::inputs_behind`] gives, in its order.
    /// A depth-first walk that visits each end once, as the plain follow
    /// does, so one walk costs work in proportion to what it can reach,
    /// and every walk after it reuses what it kept.
    ///
    /// What it keeps: when the walk finds an input, that input for every
    /// end on its path; when it finds none, a "none" for every end it
    /// reached, since each of them reaches only ends the start reaches.
    /// So inside a loop of pass-through ends with more than one way out,
    /// an end may get another input the loop reaches than the plain
    /// follow's first: which one depends on the end asked first. Keeping
    /// the exact first for every end of such a loop would cost a walk per
    /// end again, for a choice of colour in a crafted loop.
    pub fn first_input(&self, level: &ModulePath, end: &End) -> Option<Flat> {
        let mut firsts = self.firsts.borrow_mut();
        let start: Key = (level.clone(), end.clone());
        if let Some(known) = firsts.get(&start) {
            return known.clone();
        }
        /// One end being walked: its ends further on, how many were
        /// tried, and whether one was cut short by a loop or by an end
        /// this walk already met.
        struct Frame {
            key: Key,
            next: Vec<Key>,
            tried: usize,
            cut: bool,
        }
        let mut stack: Vec<Frame> = Vec::new();
        // Every end this walk opened, on the path now or finished.
        let mut seen: BTreeSet<Key> = BTreeSet::new();
        // The answer of the end just finished, handed to its parent.
        let mut pending: Option<(Option<Flat>, bool)> = None;
        let mut visit = Some(start);
        loop {
            if let Some(key) = visit.take() {
                match firsts.get(&key) {
                    Some(known) => pending = Some((known.clone(), false)),
                    None => match self.hierarchy.input_step(self.graph, &key.0, &key.1) {
                        InputStep::Found(flat) => {
                            firsts.insert(key, Some(flat.clone()));
                            pending = Some((Some(flat), false));
                        }
                        InputStep::Nothing => {
                            firsts.insert(key, None);
                            pending = Some((None, false));
                        }
                        InputStep::Next(next) => {
                            seen.insert(key.clone());
                            stack.push(Frame {
                                key,
                                next,
                                tried: 0,
                                cut: false,
                            });
                        }
                    },
                }
            }
            let Some(top) = stack.last_mut() else {
                let answer = pending.and_then(|(answer, _)| answer);
                if answer.is_none() {
                    for key in seen {
                        firsts.insert(key, None);
                    }
                }
                return answer;
            };
            if let Some((answer, cut)) = pending.take() {
                top.cut |= cut;
                if answer.is_some() {
                    // The first input found answers every end on the path.
                    while let Some(frame) = stack.pop() {
                        firsts.insert(frame.key, answer.clone());
                    }
                    return answer;
                }
            }
            if top.tried == top.next.len() {
                let frame = stack.pop().expect("the top frame");
                if !frame.cut {
                    firsts.insert(frame.key, None);
                }
                pending = Some((None, frame.cut));
                continue;
            }
            let child = top.next[top.tried].clone();
            top.tried += 1;
            if firsts.contains_key(&child) || !seen.contains(&child) {
                visit = Some(child);
            } else {
                top.cut = true;
            }
        }
    }
}

#[cfg(test)]
mod tests;
