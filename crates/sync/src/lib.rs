//! klerq-sync — real-time collaborative editing, CRDT-based.
//!
//! Gives Klerq documents Google-Docs-style live sync: every replica edits
//! locally and emits [`Op`]s; shipping those ops to peers (in any order, with
//! duplicates) makes every replica **converge** to the same state. No central
//! server or locking is required — merge is commutative and deterministic.
//!
//! Two CRDTs:
//! - [`CalcCrdt`] — a last-writer-wins map keyed by cell, ordered by a Lamport
//!   [`Stamp`] with a site tiebreak. Perfect for spreadsheets.
//! - [`TextCrdt`] — a Logoot sequence CRDT: characters carry dense, unique
//!   position identifiers so concurrent inserts interleave deterministically.
//!
//! [`Session`] bundles both plus an outbox of ops to broadcast. Ops are
//! `serde`-serializable, so any transport (WebSocket, WebRTC, a relay) works;
//! the convergence guarantees live here and are unit-tested.
//!
//! Built TDD-first — see the `tests` module.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// A replica identifier. Unique per open document per user/tab.
pub type Site = u64;

/// Lamport timestamp with a site tiebreak — a total order across replicas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub clock: u64,
    pub site: Site,
}

// ===================== Calc: LWW map =====================

/// A single cell write to broadcast.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalcOp {
    pub col: u32,
    pub row: u32,
    pub stamp: Stamp,
    /// `None` clears the cell.
    pub value: Option<String>,
}

/// Last-writer-wins grid CRDT.
#[derive(Clone, Debug, Default)]
pub struct CalcCrdt {
    site: Site,
    clock: u64,
    cells: HashMap<(u32, u32), (Stamp, Option<String>)>,
}

impl CalcCrdt {
    pub fn new(site: Site) -> Self {
        Self {
            site,
            clock: 0,
            cells: HashMap::new(),
        }
    }

    fn tick(&mut self) -> Stamp {
        self.clock += 1;
        Stamp {
            clock: self.clock,
            site: self.site,
        }
    }

    /// Local edit: set (or clear) a cell, returning the op to broadcast.
    pub fn set(&mut self, col: u32, row: u32, value: Option<String>) -> CalcOp {
        let stamp = self.tick();
        self.write(col, row, stamp, value.clone());
        CalcOp {
            col,
            row,
            stamp,
            value,
        }
    }

    /// Apply a remote op (idempotent, order-independent).
    pub fn apply(&mut self, op: &CalcOp) {
        self.clock = self.clock.max(op.stamp.clock);
        self.write(op.col, op.row, op.stamp, op.value.clone());
    }

    fn write(&mut self, col: u32, row: u32, stamp: Stamp, value: Option<String>) {
        match self.cells.entry((col, row)) {
            Entry::Occupied(mut o) => {
                if stamp > o.get().0 {
                    o.insert((stamp, value));
                }
            }
            Entry::Vacant(v) => {
                v.insert((stamp, value));
            }
        }
    }

    /// Current value of a cell (if set and not cleared).
    pub fn get(&self, col: u32, row: u32) -> Option<&str> {
        self.cells.get(&(col, row)).and_then(|(_, v)| v.as_deref())
    }

    /// Merge another replica wholesale.
    pub fn merge(&mut self, other: &CalcCrdt) {
        for (&(c, r), (s, v)) in &other.cells {
            self.clock = self.clock.max(s.clock);
            self.write(c, r, *s, v.clone());
        }
    }

    /// Non-empty cells, for materializing into a sheet.
    pub fn cells(&self) -> impl Iterator<Item = (u32, u32, &str)> {
        self.cells
            .iter()
            .filter_map(|(&(c, r), (_, v))| v.as_deref().map(|s| (c, r, s)))
    }
}

// ===================== Text: Logoot sequence =====================

/// A dense position identifier: a path of `(digit, site)` components compared
/// lexicographically. Two concurrent inserts at the same gap get distinct
/// positions (site breaks the tie), so all replicas order them the same way.
pub type Pos = Vec<(u32, Site)>;

const BASE: u32 = 1 << 16;

/// A character edit to broadcast.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextOp {
    Insert { pos: Pos, ch: char },
    Delete { pos: Pos },
}

/// Sequence CRDT for collaborative text.
#[derive(Clone, Debug, Default)]
pub struct TextCrdt {
    site: Site,
    atoms: BTreeMap<Pos, char>,
}

/// Allocate a position strictly between `l` and `r` (exclusive), owned by `site`.
fn alloc_between(l: &Pos, r: Option<&Pos>, site: Site) -> Pos {
    let mut res: Pos = Vec::new();
    let mut right = r.cloned();
    let mut i = 0;
    loop {
        let ld = l.get(i).map(|x| x.0).unwrap_or(0);
        let rd = right
            .as_ref()
            .and_then(|r| r.get(i))
            .map(|x| x.0)
            .unwrap_or(BASE);
        if rd - ld > 1 {
            // Room here: pick the next digit up, disambiguated by site.
            res.push((ld + 1, site));
            return res;
        }
        // No room: copy the left digit and descend one level.
        let s = l.get(i).map(|x| x.1).unwrap_or(site);
        res.push((ld, s));
        if ld < rd {
            // We dropped strictly below the right bound; it no longer constrains.
            right = None;
        }
        i += 1;
    }
}

impl TextCrdt {
    pub fn new(site: Site) -> Self {
        Self {
            site,
            atoms: BTreeMap::new(),
        }
    }

    pub fn text(&self) -> String {
        self.atoms.values().collect()
    }

    pub fn len(&self) -> usize {
        self.atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }

    fn nth_pos(&self, i: usize) -> Option<Pos> {
        self.atoms.keys().nth(i).cloned()
    }

    /// Local insert of `ch` at visible `index`, returning the op to broadcast.
    pub fn insert(&mut self, index: usize, ch: char) -> TextOp {
        let left = if index == 0 {
            Vec::new()
        } else {
            self.nth_pos(index - 1).unwrap_or_default()
        };
        let right = self.nth_pos(index);
        let pos = alloc_between(&left, right.as_ref(), self.site);
        self.atoms.insert(pos.clone(), ch);
        TextOp::Insert { pos, ch }
    }

    /// Insert a whole string starting at `index`; returns one op per char.
    pub fn insert_str(&mut self, index: usize, s: &str) -> Vec<TextOp> {
        s.chars()
            .enumerate()
            .map(|(k, ch)| self.insert(index + k, ch))
            .collect()
    }

    /// Local delete of the character at `index`.
    pub fn delete(&mut self, index: usize) -> Option<TextOp> {
        let pos = self.nth_pos(index)?;
        self.atoms.remove(&pos);
        Some(TextOp::Delete { pos })
    }

    /// Apply a remote op (idempotent, order-independent).
    pub fn apply(&mut self, op: &TextOp) {
        match op {
            TextOp::Insert { pos, ch } => {
                self.atoms.insert(pos.clone(), *ch);
            }
            TextOp::Delete { pos } => {
                self.atoms.remove(pos);
            }
        }
    }
}

// ===================== Session + transport-agnostic op =====================

/// A collaborative op for either document kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Calc(CalcOp),
    Text(TextOp),
}

/// One participant's live view: both CRDTs plus an outbox of ops to broadcast.
#[derive(Clone, Debug)]
pub struct Session {
    pub site: Site,
    pub calc: CalcCrdt,
    pub text: TextCrdt,
    outbox: Vec<Op>,
}

impl Session {
    pub fn new(site: Site) -> Self {
        Self {
            site,
            calc: CalcCrdt::new(site),
            text: TextCrdt::new(site),
            outbox: Vec::new(),
        }
    }

    /// Local cell edit; queues an op for peers.
    pub fn set_cell(&mut self, col: u32, row: u32, value: Option<String>) {
        let op = self.calc.set(col, row, value);
        self.outbox.push(Op::Calc(op));
    }

    /// Local text insert; queues an op for peers.
    pub fn insert_text(&mut self, index: usize, ch: char) {
        let op = self.text.insert(index, ch);
        self.outbox.push(Op::Text(op));
    }

    /// Local text delete; queues an op for peers.
    pub fn delete_text(&mut self, index: usize) {
        if let Some(op) = self.text.delete(index) {
            self.outbox.push(Op::Text(op));
        }
    }

    /// Apply an op received from a peer.
    pub fn apply_remote(&mut self, op: &Op) {
        match op {
            Op::Calc(o) => self.calc.apply(o),
            Op::Text(o) => self.text.apply(o),
        }
    }

    /// Take the queued ops to broadcast (clears the outbox).
    pub fn drain_outbox(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.outbox)
    }

    /// Serialize the outbox to JSON for any text transport.
    pub fn export_ops(&mut self) -> String {
        serde_json::to_string(&self.drain_outbox()).unwrap_or_else(|_| "[]".into())
    }

    /// Apply a JSON array of ops received from a peer.
    pub fn import_ops(&mut self, json: &str) -> Result<usize, String> {
        let ops: Vec<Op> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for op in &ops {
            self.apply_remote(op);
        }
        Ok(ops.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- Calc ----------

    #[test]
    fn calc_concurrent_write_converges_deterministically() {
        let mut a = CalcCrdt::new(1);
        let mut b = CalcCrdt::new(2);
        // Concurrent writes to the same cell.
        let oa = a.set(0, 0, Some("from-A".into()));
        let ob = b.set(0, 0, Some("from-B".into()));
        // Exchange.
        a.apply(&ob);
        b.apply(&oa);
        // Both converge to the SAME winner (higher stamp; equal clock → higher site).
        assert_eq!(a.get(0, 0), b.get(0, 0));
        assert_eq!(a.get(0, 0), Some("from-B")); // site 2 > site 1
    }

    #[test]
    fn calc_apply_order_independent() {
        let mut src = CalcCrdt::new(1);
        let o1 = src.set(0, 0, Some("x".into()));
        let o2 = src.set(0, 0, Some("y".into()));
        let o3 = src.set(1, 1, Some("z".into()));

        let mut r1 = CalcCrdt::new(9);
        for op in [&o1, &o2, &o3] {
            r1.apply(op);
        }
        let mut r2 = CalcCrdt::new(9);
        for op in [&o3, &o2, &o1] {
            r2.apply(op);
        }
        // Duplicates too.
        r2.apply(&o1);
        assert_eq!(r1.get(0, 0), r2.get(0, 0));
        assert_eq!(r1.get(0, 0), Some("y"));
        assert_eq!(r1.get(1, 1), Some("z"));
    }

    #[test]
    fn calc_clear_is_a_write() {
        let mut a = CalcCrdt::new(1);
        a.set(0, 0, Some("hi".into()));
        let clear = a.set(0, 0, None);
        let mut b = CalcCrdt::new(2);
        b.apply(&clear);
        assert_eq!(b.get(0, 0), None);
    }

    // ---------- Text ----------

    #[test]
    fn text_local_insert_reads_back() {
        let mut t = TextCrdt::new(1);
        t.insert_str(0, "hello");
        assert_eq!(t.text(), "hello");
        t.insert(5, '!');
        assert_eq!(t.text(), "hello!");
        t.delete(0);
        assert_eq!(t.text(), "ello!");
    }

    #[test]
    fn text_concurrent_insert_converges() {
        // Both start from the same base "AC", insert 'B' / 'X' between A and C.
        let mut a = TextCrdt::new(1);
        let base = a.insert_str(0, "AC");
        let mut b = TextCrdt::new(2);
        for op in &base {
            b.apply(op);
        }
        assert_eq!(a.text(), "AC");
        assert_eq!(b.text(), "AC");

        let oa = a.insert(1, 'B'); // A B C on replica a
        let ob = b.insert(1, 'X'); // A X C on replica b
        a.apply(&ob);
        b.apply(&oa);

        // Converge to the same interleaving on both replicas.
        assert_eq!(a.text(), b.text());
        assert_eq!(a.text().len(), 4);
        assert!(a.text().starts_with('A') && a.text().ends_with('C'));
    }

    #[test]
    fn text_ops_apply_in_any_order() {
        let mut src = TextCrdt::new(1);
        let ops = src.insert_str(0, "abcd");
        assert_eq!(src.text(), "abcd");

        let mut r1 = TextCrdt::new(7);
        for op in &ops {
            r1.apply(op);
        }
        let mut r2 = TextCrdt::new(7);
        for op in ops.iter().rev() {
            r2.apply(op);
        }
        r2.apply(&ops[0]); // duplicate
        assert_eq!(r1.text(), "abcd");
        assert_eq!(r2.text(), "abcd");
    }

    // ---------- Session + transport ----------

    #[test]
    fn two_sessions_converge_via_json_ops() {
        let mut alice = Session::new(1);
        let mut bob = Session::new(2);

        alice.set_cell(0, 0, Some("=SUM(A2:A3)".into()));
        alice.insert_text(0, 'H');
        alice.insert_text(1, 'i');

        // Ship Alice's ops as JSON to Bob.
        let wire = alice.export_ops();
        let n = bob.import_ops(&wire).unwrap();
        assert_eq!(n, 3);

        assert_eq!(bob.calc.get(0, 0), Some("=SUM(A2:A3)"));
        assert_eq!(bob.text.text(), "Hi");

        // Bob edits back; Alice converges.
        bob.set_cell(0, 0, Some("42".into()));
        let back = bob.export_ops();
        alice.import_ops(&back).unwrap();
        assert_eq!(alice.calc.get(0, 0), bob.calc.get(0, 0));
    }

    #[test]
    fn import_bad_ops_errors() {
        let mut s = Session::new(1);
        assert!(s.import_ops("not json").is_err());
    }
}
