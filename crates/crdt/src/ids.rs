//! Client-scoped id allocation without coordination (spec §5.1).
//!
//! The server hands each connection a 32-bit [`ClientId`] and starts its counter *above every
//! node-id space ever used in the file*, so session ids are never re-granted — the counter is part
//! of the saved [`crate::server::SessionState`], so a server restart doesn't hand out ids twice
//! either, and a reconnecting client keeps its id.
//!
//! Node ids are `client_id << 32 | counter`, continued above the highest id of that space in the
//! document. Ids in existing files (space 0) keep working untouched, and a document without a
//! client id allocates exactly as it always did.

use serde::{Deserialize, Serialize};

/// A client connection id granted by the server (32-bit).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub struct ClientId(pub u32);

/// An object id inside a document: client space in the high 32 bits, counter in the low 32.
///
/// Two clients creating a node in the same instant get different ids with zero coordination
/// (spec §5.1 P1-P4): allocation never repeats (P1), space-0 file ids work unchanged (P2),
/// allocation is O(1) with per-space next-id memory computed once at open (P3), and the server
/// refuses `Create` with an id from another client's space unless it was previously removed (P4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
pub struct NodeId(pub u64);

impl NodeId {
    /// The client space this id belongs to (`0` = legacy file ids).
    #[must_use]
    pub fn space(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// The per-space counter of this id.
    #[must_use]
    pub fn counter(self) -> u32 {
        // The low 32 bits by construction (compose packs them there).
        (self.0 & 0xFFFF_FFFF) as u32
    }

    /// Build the id for `space` and `counter`.
    #[must_use]
    pub fn compose(space: u32, counter: u32) -> Self {
        NodeId((u64::from(space) << 32) | u64::from(counter))
    }
}

/// The per-space next-counter bookkeeping a document keeps (persisted in the file).
///
/// `next` continues above the highest id of the space *in the document* — allocation is local and
/// deterministic, and the guard `next > highest seen` is maintained by [`Allocator::note`].
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allocator {
    next: std::collections::HashMap<u32, u64>,
}

impl Allocator {
    /// An empty allocator (a fresh local document).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Note an id that exists in the document, so future allocation stays above it.
    pub fn note(&mut self, id: NodeId) {
        let e = self.next.entry(id.space()).or_insert(0);
        if u64::from(id.counter()) >= *e {
            *e = u64::from(id.counter()) + 1;
        }
    }

    /// Allocate the next id in `space`, when the space has one left.
    ///
    /// O(1), and never repeats within the space (spec §5.1 P1). `None` once the space is
    /// exhausted at 2^32 ids.
    ///
    /// The counter is 32 bits wide because a [`NodeId`] packs it beside the space. Masking a
    /// wider counter down to fit kept the promise of P1 only until the first wrap, and then
    /// quietly broke it: the next id was one this space had already issued, and
    /// [`crate::text::TextDoc::apply`] drops a known id as a duplicate delivery. So an
    /// exhausted space refuses instead. A caller out of ids asks the server for another space,
    /// which is what [`Allocator::next_client_id`] is for.
    pub fn alloc(&mut self, space: u32) -> Option<NodeId> {
        let counter = self.next.entry(space).or_insert(0);
        let next = u32::try_from(*counter).ok()?;
        *counter += 1;
        Some(NodeId::compose(space, next))
    }

    /// The server-side client counter that starts above every id space used in the file.
    ///
    /// `used_spaces` are the client spaces present in the document (nodes, styles, variables,
    /// collections, comments, modes — every doc-level entry shares the same allocator, spec §2.2).
    #[must_use]
    pub fn next_client_id(&self, used_spaces: &[u32], granted_so_far: u32) -> Option<ClientId> {
        let highest = used_spaces.iter().copied().chain([granted_so_far]).max()?;
        if highest == u32::MAX {
            return None; // Exhausted: 4 billion client spaces — the file refuses new sessions.
        }
        Some(ClientId(highest + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec §5.1 P1: no id is ever allocated twice in a space.
    #[test]
    fn never_repeats() {
        let mut a = Allocator::new();
        let first = a.alloc(7).expect("a fresh space has ids");
        let second = a.alloc(7).expect("a fresh space has ids");
        assert_ne!(first, second);
        assert_eq!(first.space(), 7);
        assert_eq!(second.counter(), first.counter() + 1);
    }

    /// An exhausted space refuses rather than reissuing its first id. Masking a wider counter
    /// into the 32 bits a `NodeId` carries wrapped silently, and a reissued id is dropped as a
    /// duplicate delivery by every replica that already holds it.
    #[test]
    fn an_exhausted_space_refuses() {
        let mut a = Allocator::new();
        a.note(NodeId::compose(4, u32::MAX));
        assert_eq!(a.alloc(4), None);
        // And a different space is untouched by its neighbour running out.
        assert_eq!(a.alloc(5).map(NodeId::counter), Some(0));
    }

    /// Spec §5.1 P2: space-0 file ids continue working — allocation above a noted id.
    #[test]
    fn continues_above_existing() {
        let mut a = Allocator::new();
        a.note(NodeId::compose(0, 41));
        assert_eq!(a.alloc(0).map(NodeId::counter), Some(42));
        // Other spaces are independent.
        assert_eq!(a.alloc(3).map(NodeId::counter), Some(0));
    }

    /// Spec §5.1 P3: allocation is O(1); noted ids never move backwards.
    #[test]
    fn note_never_lowers() {
        let mut a = Allocator::new();
        a.note(NodeId::compose(2, 100));
        a.note(NodeId::compose(2, 5));
        assert_eq!(a.alloc(2).map(NodeId::counter), Some(101));
    }

    /// The server grants client ids above every space used in the file.
    #[test]
    fn client_ids_start_above_used_spaces() {
        let a = Allocator::new();
        assert_eq!(a.next_client_id(&[3, 9], 0), Some(ClientId(10)));
        assert_eq!(a.next_client_id(&[3, 9], 12), Some(ClientId(13)));
        assert_eq!(a.next_client_id(&[], 0), Some(ClientId(1)));
        assert_eq!(a.next_client_id(&[u32::MAX], 0), None);
    }
}
