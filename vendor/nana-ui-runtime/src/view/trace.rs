//! Causal trace of the reactive runtime: which write made which node update.
//!
//! Records are ids and `&'static` pointers written into a ring allocated
//! once per thread, so recording never allocates, formats or does I/O. A
//! query ([`AppContext::why_updated`](crate::AppContext::why_updated)) walks
//! the ring off the frame path.

use std::panic::Location;

use super::node::SourceLocation;
use super::reactive::{EffectKey, SignalKey};
use crate::StableNodeId;

const CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Record {
    Write {
        epoch: u64,
        signal: SignalKey,
        at: &'static Location<'static>,
        created: Option<&'static Location<'static>>,
    },
    Notify {
        epoch: u64,
        signal: SignalKey,
        effect: EffectKey,
    },
    Patch {
        epoch: u64,
        node: StableNodeId,
        effect: EffectKey,
    },
}

/// Writes between two flush rounds belong to the round that follows them.
pub(crate) struct Ring {
    records: Vec<Record>,
    next: usize,
    epoch: u64,
}

impl Ring {
    pub(crate) fn new() -> Self {
        Self {
            records: Vec::with_capacity(CAPACITY),
            next: 0,
            epoch: 1,
        }
    }

    fn push(&mut self, record: Record) {
        if self.records.len() < CAPACITY {
            self.records.push(record);
        } else {
            self.records[self.next] = record;
        }
        self.next = (self.next + 1) % CAPACITY;
    }

    pub(crate) fn write(
        &mut self,
        signal: SignalKey,
        at: &'static Location<'static>,
        created: Option<&'static Location<'static>>,
    ) {
        let epoch = self.epoch;
        self.push(Record::Write {
            epoch,
            signal,
            at,
            created,
        });
    }

    pub(crate) fn notify(&mut self, signal: SignalKey, effect: EffectKey) {
        let epoch = self.epoch;
        self.push(Record::Notify {
            epoch,
            signal,
            effect,
        });
    }

    /// A flush round starts: what it patches is caused by the writes recorded
    /// under the current epoch; later writes belong to the next round.
    pub(crate) fn begin_round(&mut self) -> u64 {
        let round = self.epoch;
        self.epoch += 1;
        round
    }

    pub(crate) fn patch(&mut self, round: u64, node: StableNodeId, effect: EffectKey) {
        self.push(Record::Patch {
            epoch: round,
            node,
            effect,
        });
    }

    /// Newest first.
    fn newest_first(&self) -> impl Iterator<Item = &Record> {
        let (older, newer) = self.records.split_at(self.next.min(self.records.len()));
        newer.iter().chain(older.iter()).rev()
    }

    /// The last patch of `node` and the writes that caused it.
    pub(crate) fn why(&self, node: StableNodeId) -> Option<Vec<Cause>> {
        let (round, effect) = self.newest_first().find_map(|record| match *record {
            Record::Patch {
                epoch,
                node: patched,
                effect,
            } if patched == node => Some((epoch, effect)),
            _ => None,
        })?;
        let signals: Vec<SignalKey> = self
            .newest_first()
            .filter_map(|record| match *record {
                Record::Notify {
                    epoch,
                    signal,
                    effect: notified,
                } if epoch == round && notified == effect => Some(signal),
                _ => None,
            })
            .collect();
        let mut causes: Vec<Cause> = self
            .newest_first()
            .filter_map(|record| match *record {
                Record::Write {
                    epoch,
                    signal,
                    at,
                    created,
                } if epoch == round && signals.contains(&signal) => Some(Cause {
                    signal_created: created,
                    written_at: at,
                }),
                _ => None,
            })
            .collect();
        causes.reverse();
        Some(causes)
    }
}

/// One write behind an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cause {
    /// Where the signal was created (`signal(..)`), if it still existed.
    pub signal_created: Option<&'static Location<'static>>,
    /// Where it was written (`set` / `update`).
    pub written_at: &'static Location<'static>,
}

/// Why a node was last patched: the element, the fields its bindings write,
/// and the writes that queued it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyUpdated {
    pub node: StableNodeId,
    /// Where the element was declared.
    pub element: &'static Location<'static>,
    /// Each bound field and where its binding was declared.
    pub bindings: Vec<(&'static str, &'static Location<'static>)>,
    pub source_element: Option<SourceLocation>,
    pub source_bindings: Vec<(&'static str, SourceLocation)>,
    pub causes: Vec<Cause>,
}
