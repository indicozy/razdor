//! «Максимум» thinking ahead (Razdor's, the owner's request): the battle on screen keeps one
//! [`MaxThinker`] for the computer's side. The search for a decision runs on a worker thread
//! and starts as soon as the position it is for is known: right after the action before it
//! is applied (while that action's animation plays), or, while the player is to act, in the
//! position after the reply the search expects of him (pondering). The battle never waits
//! for it: when the move is needed in another position, that search is stopped and a new one
//! starts (the tables keep what the old one found).

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

use super::*;

/// The most a search runs before the battle needs its move (a ponder while the player
/// thinks): it then waits with its answer.
const PONDER_LIMIT: Duration = Duration::from_secs(3);
/// The player's moves played ahead to find the position of the next decision, at most.
const PREDICT_STEPS: usize = 16;

/// What [`MaxThinker::poll`] has for the battle's move now.
#[derive(Clone, Copy, Debug)]
pub enum MaxPoll {
    /// Still searching (the clock pointer shows).
    Thinking,
    Ready(MaxReport),
    /// The worker failed: the original's AI moves instead.
    Failed,
}

/// A search on the worker for one position.
struct Job {
    key: StateKey,
    control: Arc<MaxControl>,
    answer: Receiver<Option<MaxReport>>,
    report: Option<MaxReport>,
    /// The battle needs this position's move (its deadline is set).
    needed: bool,
    /// When the battle first asked for it.
    asked: Option<Instant>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.control.stop();
    }
}

/// «Максимум» for one side of one battle: its tables, kept for the whole battle, and the
/// search under way.
pub struct MaxThinker {
    side: Team,
    tables: Arc<MaxTables>,
    job: Option<Job>,
    /// How long the battle waited for each move (from its first ask to the answer).
    pub pauses: Vec<Duration>,
    /// The root's moves on several threads ([`MaxBudget::parallel`]).
    parallel: bool,
}

impl MaxThinker {
    pub fn new(side: Team) -> MaxThinker {
        MaxThinker { side, tables: Arc::new(MaxTables::default()), job: None, pauses: Vec::new(), parallel: true }
    }

    /// Searches on one thread only (to measure the threads).
    pub fn set_parallel(&mut self, on: bool) {
        self.parallel = on;
    }

    pub fn side(&self) -> Team {
        self.side
    }

    pub fn tables(&self) -> &MaxTables {
        &self.tables
    }

    /// The position of this side's next decision as the battle `b` stands, with this
    /// thread's patch globals as they will be then: `b` itself when one of its units is to
    /// act, else after the other side's moves the search expects (the table's best move, or
    /// the original AI's where the table has none). `None` if the battle ends first. `b` and
    /// the globals are left as they are.
    fn expected(&self, b: &Battle) -> Option<(Battle, PatchGlobals)> {
        let (saved, silent) = (patch_globals(), crate::i18n::set_silent(true));
        let mut r = b.clone();
        r.log.clear();
        let mut found = None;
        for _ in 0..PREDICT_STEPS {
            let Some(id) = r.active() else { break };
            if r.fighters[id].team == self.side {
                found = Some(patch_globals());
                break;
            }
            let legal = r.max_moves(id, false);
            let guess = self.tables.tt_get(r.max_key(self.side)).and_then(|e| e.best).filter(|m| m.0.is_none_or(|p| legal.contains(&p)));
            let m = guess.unwrap_or_else(|| r.original_move(&legal));
            r.play_max_move(m);
        }
        set_patch_globals(saved);
        crate::i18n::set_silent(silent);
        found.map(|g| (r, g))
    }

    fn start(&mut self, root: Battle, globals: PatchGlobals, key: StateKey) {
        self.job = None;
        let control = Arc::new(MaxControl::new(PONDER_LIMIT));
        let (tx, answer) = std::sync::mpsc::channel();
        let (tables, c, parallel) = (self.tables.clone(), control.clone(), self.parallel);
        let spawned = std::thread::Builder::new().name("max-ai".into()).spawn(move || {
            set_patch_globals(globals);
            let r = root.max_choice_with(MaxBudget { time: None, parallel, ..MaxBudget::GAME }, Some(&tables), Some(&c));
            let _ = tx.send(r);
        });
        if spawned.is_ok() {
            self.job = Some(Job { key, control, answer, report: None, needed: false, asked: None });
        }
    }

    /// Starts searching the next decision ahead of the need, if that is not under way
    /// already: call after each action is applied (it is cheap when nothing changed).
    pub fn think_ahead(&mut self, b: &Battle) {
        let Some((root, globals)) = self.expected(b) else { return };
        let saved = patch_globals();
        set_patch_globals(globals);
        let key = root.max_key(self.side);
        set_patch_globals(saved);
        if self.job.as_ref().is_some_and(|j| j.key == key) {
            return;
        }
        self.start(root, globals, key);
    }

    /// The battle needs the move of its active unit (of this side) now: the answer when the
    /// search for this very position has one, else `Thinking` (a search for any other
    /// position is dropped and this one's started). The search ends [`MAX_AI_BUDGET`] after
    /// it began, or at once when it began that long ago.
    pub fn poll(&mut self, b: &Battle) -> MaxPoll {
        let key = b.max_key(self.side);
        if self.job.as_ref().is_none_or(|j| j.key != key) {
            let mut root = b.clone();
            root.log.clear();
            self.start(root, patch_globals(), key);
        }
        let Some(j) = self.job.as_mut() else { return MaxPoll::Failed };
        let asked = *j.asked.get_or_insert_with(Instant::now);
        if !j.needed {
            j.needed = true;
            j.control.need(MAX_AI_BUDGET);
        }
        if j.report.is_none() {
            match j.answer.try_recv() {
                Ok(Some(r)) => j.report = Some(r),
                Err(TryRecvError::Empty) => return MaxPoll::Thinking,
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    return MaxPoll::Failed;
                }
            }
        }
        let report = self.job.take().and_then(|mut j| j.report.take());
        self.pauses.push(asked.elapsed());
        report.map_or(MaxPoll::Failed, MaxPoll::Ready)
    }

    /// Stops the search under way (the battle is over, or nobody needs it).
    pub fn rest(&mut self) {
        self.job = None;
    }
}
