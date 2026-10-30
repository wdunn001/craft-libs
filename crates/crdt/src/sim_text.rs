//! The text convergence simulator: N replicas writing in one paragraph over lossy links.
//!
//! [`crate::sim`] proves the change algebra converges. It never touches [`TextDoc`], and the
//! text is the part that rides the CollabCraft wire. Four divergence defects were found by
//! hand in the layer above this crate, and every one of them would have failed here.
//!
//! What this models, which the change-algebra simulator does not:
//!
//! * **A shared base loaded from a file.** Every replica seeds the same string in space 0, the
//!   way an application loads a saved paragraph. Identities have to come out the same on each.
//! * **One id space per replica**, granted rather than guessed. Two replicas minting in one
//!   space produce the same id for different characters, and [`TextDoc::apply`] drops the
//!   second as a duplicate delivery.
//! * **A late joiner.** It seeds the file and is replayed the operations it missed. The file is
//!   behind the room, so a replica that only seeds diverges.
//! * **Delays, drops, duplicate delivery and reordering.** RGA tolerates all four. A test that
//!   delivers in order proves none of it.

use std::collections::HashMap;

use crate::ids::Allocator;
use crate::sim::Rng;
use crate::text::{TextDoc, TextOp};

/// One replica writing in one text unit.
pub struct TextPeer {
    /// The id space this replica mints in. Never 0: that space is the file's.
    pub space: u32,
    /// The replicated text.
    pub doc: TextDoc,
    /// This replica's own counters.
    alloc: Allocator,
    /// Whether it is connected. A disconnected replica keeps writing and queues what it sends.
    pub connected: bool,
    queued: Vec<TextOp>,
}

impl TextPeer {
    /// A replica seeded from the file's text, minting new ids in `space`.
    #[must_use]
    pub fn seeded(space: u32, base: &str) -> TextPeer {
        let mut alloc = Allocator::new();
        let mut doc = TextDoc::new();
        if !base.is_empty() {
            // Space 0 from a fresh allocator, so every replica that loads the same string
            // reaches the same identities with nothing passed between them.
            let _ = doc.local_insert(0, base, &mut alloc, 0);
        }
        TextPeer { space: space.max(1), doc, alloc, connected: true, queued: Vec::new() }
    }

    /// A replica joining a session that is already running.
    ///
    /// It loads the file, then applies the operations the room has already seen. The replay
    /// carries ids from other spaces, which never collide with this replica's own.
    #[must_use]
    pub fn joining(space: u32, base: &str, replay: &[TextOp]) -> TextPeer {
        let mut peer = TextPeer::seeded(space, base);
        for op in replay {
            peer.doc.apply(op);
        }
        peer
    }
}

/// One operation in flight on a link.
struct InFlight {
    deliver_at: u64,
    /// Which replica sent it, so it is not delivered back to the sender.
    from: usize,
    op: TextOp,
}

/// N replicas, one text unit, and the links between them.
pub struct TextWorld {
    pub peers: Vec<TextPeer>,
    /// The file's text, which a late joiner also loads.
    pub base: String,
    /// Every operation the room has produced, in the order the links accepted them.
    ///
    /// This is the relay's retained log, and what a joining replica is replayed.
    pub log: Vec<TextOp>,
    in_flight: Vec<InFlight>,
    time: u64,
    rng: Rng,
}

impl TextWorld {
    /// A world of `n` replicas over `base`, seeded for reproducibility.
    #[must_use]
    pub fn new(n: usize, base: &str, seed: u64) -> TextWorld {
        // Spaces 1..=n, the way a relay hands one out per connection.
        let peers = (1..=n).map(|i| TextPeer::seeded(u32::try_from(i).unwrap_or(1), base)).collect();
        TextWorld { peers, base: base.to_owned(), log: Vec::new(), in_flight: Vec::new(), time: 0, rng: Rng::new(seed) }
    }

    /// One random edit from replica `pi`, sent to everybody else.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub fn random_edit(&mut self, pi: usize) {
        let live = self.peers.get(pi).map_or(0, |p| p.doc.render().chars().count());
        let insert = live == 0 || self.rng.chance(2, 3);
        let ops = {
            let pos = if live == 0 { 0 } else { usize::try_from(self.rng.below(live as u64 + 1)).unwrap_or(0) };
            let Some(peer) = self.peers.get_mut(pi) else { return };
            if insert {
                // A short run, because a human types runs and a one-character test hides the
                // contiguity rule.
                let len = 1 + self.rng.below(3);
                let text: String = (0..len).map(|k| char::from(b'a' + u8::try_from((self.rng.below(26) + k) % 26).unwrap_or(0))).collect();
                peer.doc.local_insert(pos, &text, &mut peer.alloc, peer.space)
            } else {
                let len = usize::try_from(1 + self.rng.below(2)).unwrap_or(1);
                peer.doc.local_delete(pos.min(live.saturating_sub(1)), len)
            }
        };
        self.send(pi, ops);
    }

    /// Put one replica's operations on the links.
    pub fn send(&mut self, pi: usize, ops: Vec<TextOp>) {
        let Some(peer) = self.peers.get_mut(pi) else { return };
        if !peer.connected {
            peer.queued.extend(ops);
            return;
        }
        for op in ops {
            self.log.push(op.clone());
            let delay = 1 + self.rng.below(9);
            self.in_flight.push(InFlight { deliver_at: self.time + delay, from: pi, op: op.clone() });
            // Duplicate delivery, sometimes. RGA has to be idempotent, and a redelivered
            // insert that is applied twice is a doubled character.
            if self.rng.chance(1, 12) {
                let again = 1 + self.rng.below(14);
                self.in_flight.push(InFlight { deliver_at: self.time + again, from: pi, op });
            }
        }
    }

    /// Advance one tick. Whatever has arrived is applied, in arrival order and no other.
    pub fn tick(&mut self) {
        self.time += 1;
        let mut arrived = Vec::new();
        let mut still = Vec::new();
        for f in std::mem::take(&mut self.in_flight) {
            if f.deliver_at > self.time {
                still.push(f);
            } else if self.rng.chance(1, 25) {
                // Dropped, and retried later. Nothing is lost on this link.
                let delay = 1 + self.rng.below(6);
                still.push(InFlight { deliver_at: self.time + delay, from: f.from, op: f.op });
            } else {
                arrived.push(f);
            }
        }
        self.in_flight = still;
        for f in arrived {
            for (pi, peer) in self.peers.iter_mut().enumerate() {
                if pi == f.from || !peer.connected {
                    continue;
                }
                peer.doc.apply(&f.op);
            }
        }
    }

    /// Disconnect or reconnect one replica at random.
    ///
    /// A disconnected replica keeps writing. Its operations go out when it comes back, which is
    /// the case that puts a burst of old edits into a room that has moved on.
    pub fn churn(&mut self) {
        if self.peers.is_empty() {
            return;
        }
        let pi = usize::try_from(self.rng.below(self.peers.len() as u64)).unwrap_or(0);
        let connected = self.peers.get(pi).is_some_and(|p| p.connected);
        if connected && self.rng.chance(1, 40) {
            if let Some(peer) = self.peers.get_mut(pi) {
                peer.connected = false;
            }
        } else if !connected && self.rng.chance(1, 3) {
            self.reconnect(pi);
        }
    }

    /// Bring one replica back: catch it up on the log, then send what it wrote while away.
    pub fn reconnect(&mut self, pi: usize) {
        let log = self.log.clone();
        let queued = {
            let Some(peer) = self.peers.get_mut(pi) else { return };
            peer.connected = true;
            // The catch-up. A replica that missed operations cannot be left to believe it is
            // current, which is exactly the bug the relay's null-document resync had.
            for op in &log {
                peer.doc.apply(op);
            }
            std::mem::take(&mut peer.queued)
        };
        self.send(pi, queued);
    }

    /// Admit a replica that was not here when the session started.
    ///
    /// It loads the file and is replayed the log, which is what the relay now sends as
    /// `text_ops`. Returns its index.
    pub fn join_late(&mut self) -> usize {
        let space = u32::try_from(self.peers.len() + 1).unwrap_or(1);
        let peer = TextPeer::joining(space, &self.base, &self.log);
        self.peers.push(peer);
        self.peers.len() - 1
    }

    /// Deliver everything still in flight and reconnect everybody.
    pub fn settle(&mut self, max_ticks: usize) {
        for _ in 0..max_ticks {
            for pi in 0..self.peers.len() {
                if !self.peers[pi].connected {
                    self.reconnect(pi);
                }
            }
            if self.in_flight.is_empty() {
                break;
            }
            self.tick();
        }
        // Every replica sees the whole log once more. Idempotence makes that a no-op on
        // anything already applied, and it closes any op a drop retried past the last tick.
        let log = self.log.clone();
        for peer in &mut self.peers {
            for op in &log {
                peer.doc.apply(op);
            }
        }
    }

    /// What every replica renders, deduplicated. One entry means they converged.
    #[must_use]
    pub fn renderings(&self) -> Vec<String> {
        let mut out: Vec<String> = self.peers.iter().map(|p| p.doc.render()).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Whether every replica renders the same string.
    #[must_use]
    pub fn converged(&self) -> bool {
        self.renderings().len() <= 1
    }
}

/// How many of each character a string holds.
#[must_use]
pub fn histogram(text: &str) -> HashMap<char, usize> {
    let mut out = HashMap::new();
    for c in text.chars() {
        *out.entry(c).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const BASE: &str = "the quick brown fox";

    /// The central guarantee. Everybody writing in one paragraph converges, over links that
    /// delay, drop, duplicate and reorder.
    #[test]
    fn concurrent_writing_converges() {
        for seed in 1..=200u64 {
            let mut w = TextWorld::new(5, BASE, seed);
            for step in 0..80 {
                w.random_edit(step % 5);
                w.tick();
                w.churn();
            }
            w.settle(400);
            assert!(w.converged(), "divergence at seed {seed}: {:?}", w.renderings());
        }
    }

    /// A replica that joins after the session started reaches the same text. It loads the file
    /// and is replayed the log, and the file alone is not enough.
    #[test]
    fn a_late_joiner_converges_with_the_room() {
        for seed in 1..=100u64 {
            let mut w = TextWorld::new(3, BASE, seed);
            for step in 0..50 {
                w.random_edit(step % 3);
                w.tick();
            }
            let late = w.join_late();
            for step in 0..20 {
                w.random_edit(step % 3);
                w.tick();
            }
            w.settle(300);
            assert!(w.converged(), "divergence at seed {seed} with a late joiner: {:?}", w.renderings());
            // And the joiner is not merely equal to the others by being equally empty.
            assert!(!w.peers[late].doc.render().is_empty(), "seed {seed}");
        }
    }

    /// Seeding the file is not enough on its own. Without the replay the joiner holds the old
    /// text, which is the defect the relay's `text_ops` message exists to close.
    #[test]
    fn seeding_the_file_alone_leaves_a_joiner_behind() {
        let mut w = TextWorld::new(2, BASE, 7);
        for step in 0..30 {
            w.random_edit(step % 2);
            w.tick();
        }
        w.settle(200);
        let room = w.peers[0].doc.render();
        assert_ne!(room, BASE, "the room moved on from the file");

        let file_only = TextPeer::seeded(99, BASE);
        assert_eq!(file_only.doc.render(), BASE);
        assert_ne!(file_only.doc.render(), room, "a joiner that only seeds is behind");

        let with_replay = TextPeer::joining(99, BASE, &w.log);
        assert_eq!(with_replay.doc.render(), room, "a joiner that is replayed catches up");
    }

    /// Two replicas minting in one id space is the defect that cost the most to find. The same
    /// id for two different characters is dropped as a duplicate delivery, so each replica
    /// keeps its own character and loses the other's.
    #[test]
    fn one_id_space_for_two_replicas_diverges() {
        let mut a = TextPeer::seeded(1, "");
        let mut b = TextPeer::seeded(1, "");
        let ops_a = a.doc.local_insert(0, "A", &mut a.alloc, a.space);
        let ops_b = b.doc.local_insert(0, "B", &mut b.alloc, b.space);
        for op in &ops_b {
            a.doc.apply(op);
        }
        for op in &ops_a {
            b.doc.apply(op);
        }
        assert_ne!(a.doc.render(), b.doc.render(), "this is the failure a granted space prevents");

        // Granted spaces, same edits, converged.
        let mut c = TextPeer::seeded(1, "");
        let mut d = TextPeer::seeded(2, "");
        let ops_c = c.doc.local_insert(0, "A", &mut c.alloc, c.space);
        let ops_d = d.doc.local_insert(0, "B", &mut d.alloc, d.space);
        for op in &ops_d {
            c.doc.apply(op);
        }
        for op in &ops_c {
            d.doc.apply(op);
        }
        assert_eq!(c.doc.render(), d.doc.render());
    }

    /// Nothing anybody typed is lost, and nothing is doubled by a redelivery. Convergence
    /// alone would be satisfied by every replica losing the same characters.
    #[test]
    fn no_character_is_lost_or_doubled() {
        for seed in 1..=60u64 {
            let mut w = TextWorld::new(4, BASE, seed);
            for step in 0..60 {
                w.random_edit(step % 4);
                w.tick();
            }
            w.settle(300);
            let first = w.peers[0].doc.render();
            for (pi, peer) in w.peers.iter().enumerate() {
                assert_eq!(histogram(&peer.doc.render()), histogram(&first), "replica {pi} differs at seed {seed}");
            }
        }
    }

    /// Delivering the same operations in a different order has to reach the same text. The
    /// relay orders what it fans out; the merge may not depend on that.
    #[test]
    fn delivery_order_does_not_change_the_result() {
        for seed in 1..=60u64 {
            let mut w = TextWorld::new(3, BASE, seed);
            for step in 0..40 {
                w.random_edit(step % 3);
                w.tick();
            }
            w.settle(200);
            let expected = w.peers[0].doc.render();

            // One replica, the same log, reversed.
            let mut reversed: Vec<TextOp> = w.log.clone();
            reversed.reverse();
            // The log is applied twice: reversed, then forward. A reversed insert can arrive
            // before the anchor it names, and RGA renders an unknown anchor at the head level
            // until the anchor is known.
            let mut shuffled = TextPeer::joining(51, BASE, &reversed);
            for op in &w.log {
                shuffled.doc.apply(op);
            }
            assert_eq!(shuffled.doc.render(), expected, "seed {seed}");
        }
    }
}
