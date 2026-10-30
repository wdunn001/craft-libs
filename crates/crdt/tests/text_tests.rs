//! Properties of the RGA text type, over generated operation sequences.
//!
//! `proptest` was a declared dev-dependency of this crate that nothing used. These are the
//! properties the hand-written unit tests assert for one example each, stated once for every
//! input the generator reaches.

use craft_crdt::ids::{Allocator, NodeId};
use craft_crdt::text::{TextDoc, TextOp};
use proptest::prelude::*;

/// One scripted edit, before it is turned into operations against a real document.
#[derive(Clone, Debug)]
enum Edit {
    Insert { at: usize, text: String },
    Delete { at: usize, len: usize },
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (0usize..40, "[a-z]{1,4}").prop_map(|(at, text)| Edit::Insert { at, text }),
        (0usize..40, 1usize..4).prop_map(|(at, len)| Edit::Delete { at, len }),
    ]
}

/// A replica: a document, its own counters, and its own id space.
struct Replica {
    doc: TextDoc,
    alloc: Allocator,
    space: u32,
}

impl Replica {
    fn new(space: u32, base: &str) -> Replica {
        let mut r = Replica { doc: TextDoc::new(), alloc: Allocator::new(), space };
        if !base.is_empty() {
            // Space 0, from this replica's own allocator. Two replicas loading the same string
            // therefore reach the same identities with nothing passed between them.
            let _ = r.doc.local_insert(0, base, &mut r.alloc, 0);
        }
        r
    }

    /// Run one scripted edit, clamped to the live length, and return the operations.
    fn run(&mut self, e: &Edit) -> Vec<TextOp> {
        let live = self.doc.render().chars().count();
        match e {
            Edit::Insert { at, text } => {
                let pos = (*at).min(live);
                self.doc.local_insert(pos, text, &mut self.alloc, self.space)
            }
            Edit::Delete { at, len } => {
                if live == 0 {
                    return Vec::new();
                }
                let pos = (*at).min(live - 1);
                self.doc.local_delete(pos, *len)
            }
        }
    }

    fn apply_all(&mut self, ops: &[TextOp]) {
        for op in ops {
            self.doc.apply(op);
        }
    }
}

fn histogram(text: &str) -> std::collections::BTreeMap<char, usize> {
    let mut out = std::collections::BTreeMap::new();
    for c in text.chars() {
        *out.entry(c).or_insert(0) += 1;
    }
    out
}

proptest! {
    /// Two replicas editing concurrently converge once each has seen the other's operations.
    /// Neither replica's own edits are disturbed by the merge.
    #[test]
    fn two_replicas_converge(base in "[a-z ]{0,20}", a_edits in prop::collection::vec(edit(), 1..8), b_edits in prop::collection::vec(edit(), 1..8)) {
        let mut a = Replica::new(1, &base);
        let mut b = Replica::new(2, &base);
        prop_assert_eq!(a.doc.render(), b.doc.render(), "the same file gives the same starting text");

        let mut from_a = Vec::new();
        for e in &a_edits {
            from_a.extend(a.run(e));
        }
        let mut from_b = Vec::new();
        for e in &b_edits {
            from_b.extend(b.run(e));
        }

        a.apply_all(&from_b);
        b.apply_all(&from_a);
        prop_assert_eq!(a.doc.render(), b.doc.render());
    }

    /// Applying the same operations twice changes nothing. A relay re-sends after a reconnect,
    /// so a replica sees its share of redeliveries.
    #[test]
    fn redelivery_is_idempotent(base in "[a-z ]{0,20}", edits in prop::collection::vec(edit(), 1..8)) {
        let mut author = Replica::new(1, &base);
        let mut ops = Vec::new();
        for e in &edits {
            ops.extend(author.run(e));
        }

        let mut peer = Replica::new(2, &base);
        peer.apply_all(&ops);
        let once = peer.doc.render();
        peer.apply_all(&ops);
        peer.apply_all(&ops);
        prop_assert_eq!(peer.doc.render(), once);
    }

    /// Delivery order does not decide the result. The relay orders what it fans out, and the
    /// merge may not depend on that order.
    #[test]
    fn order_of_delivery_does_not_matter(base in "[a-z ]{0,20}", edits in prop::collection::vec(edit(), 1..8)) {
        let mut author = Replica::new(1, &base);
        let mut ops = Vec::new();
        for e in &edits {
            ops.extend(author.run(e));
        }

        let mut forward = Replica::new(2, &base);
        forward.apply_all(&ops);

        // Reversed, then forward. An insert whose anchor has not arrived renders at the head
        // level until it has, so the second pass is what settles it.
        let mut reversed_ops = ops.clone();
        reversed_ops.reverse();
        let mut backward = Replica::new(3, &base);
        backward.apply_all(&reversed_ops);
        backward.apply_all(&ops);

        prop_assert_eq!(backward.doc.render(), forward.doc.render());
    }

    /// Nothing anybody typed is lost, and nothing is doubled. Convergence on its own would be
    /// satisfied by two replicas losing the same characters.
    #[test]
    fn no_character_is_lost_or_doubled(base in "[a-z ]{0,20}", a_edits in prop::collection::vec(edit(), 1..6), b_edits in prop::collection::vec(edit(), 1..6)) {
        let mut a = Replica::new(1, &base);
        let mut b = Replica::new(2, &base);
        let mut from_a = Vec::new();
        for e in &a_edits {
            from_a.extend(a.run(e));
        }
        let mut from_b = Vec::new();
        for e in &b_edits {
            from_b.extend(b.run(e));
        }
        a.apply_all(&from_b);
        b.apply_all(&from_a);
        prop_assert_eq!(histogram(&a.doc.render()), histogram(&b.doc.render()));
    }

    /// The order a replica loads its scopes in may not change the identities any one of them
    /// receives. One allocator shared across scopes made it change them, and an anchor then
    /// meant two different characters on the two replicas.
    #[test]
    fn seeding_order_does_not_change_identities(first in "[a-z]{1,10}", second in "[a-z]{1,10}") {
        // Forward: the first scope, then the second.
        let mut a_one = Replica::new(1, &first);
        let a_two = Replica::new(1, &second);

        // Reversed: the second scope, then the first. Each scope keeps its own allocator, so
        // the order cannot reach the ids.
        let b_two = Replica::new(2, &second);
        let mut b_one = Replica::new(2, &first);

        let ids_a: Vec<NodeId> = a_two.doc.ids().collect();
        let ids_b: Vec<NodeId> = b_two.doc.ids().collect();
        prop_assert_eq!(ids_a, ids_b, "the second scope's ids must not depend on the first");

        // And an edit anchored in one scope lands in the same place on the other replica.
        let ops = a_one.doc.local_insert(first.chars().count(), "!", &mut a_one.alloc, a_one.space);
        b_one.apply_all(&ops);
        prop_assert_eq!(b_one.doc.render(), a_one.doc.render());
    }
}
