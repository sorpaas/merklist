use crate::traits::{MerkleDB, EndOf, Value, ValueOf, RootStatus, OwnedRoot, DanglingRoot, Leak};
use crate::empty::MerkleEmpty;
use crate::raw::MerkleRaw;
use crate::index::MerkleIndex;

const ROOT_INDEX: MerkleIndex = MerkleIndex::root();
const EXTEND_INDEX: MerkleIndex = MerkleIndex::root().left();

/// `MerkleTuple` with owned root.
pub type OwnedMerkleTuple<DB> = MerkleTuple<OwnedRoot, DB>;

/// `MerkleTuple` with dangling root.
pub type DanglingMerkleTuple<DB> = MerkleTuple<DanglingRoot, DB>;

/// Binary merkle tuple.
pub struct MerkleTuple<R: RootStatus, DB: MerkleDB> {
    raw: MerkleRaw<R, DB>,
    empty: MerkleEmpty<OwnedRoot, DB>,
    len: usize,
}

impl<R: RootStatus, DB: MerkleDB> MerkleTuple<R, DB> {
    fn raw_index(&self, i: usize) -> MerkleIndex {
        MerkleIndex::from_one(self.max_len() + i).expect("Got usize must be greater than 0")
    }

    fn extend(&mut self, db: &mut DB) {
        self.empty.extend(db);
        let root = self.root();
        let mut new_raw = MerkleRaw::default();
        new_raw.set(db, EXTEND_INDEX, root);
        self.raw.set(db, ROOT_INDEX, Value::End(Default::default()));
        self.raw = new_raw;
    }

    fn shrink(&mut self, db: &mut DB) {
        self.empty.shrink(db);
        match self.raw.get(db, EXTEND_INDEX) {
            Some(extended_value) => { self.raw.set(db, ROOT_INDEX, extended_value); },
            None => { self.raw.set(db, ROOT_INDEX, Value::End(Default::default())); },
        }
    }

    fn max_len(&self) -> usize {
        crate::utils::next_power_of_two(self.len())
    }

    /// Get value at index.
    pub fn get(&self, db: &DB, index: usize) -> EndOf<DB> {
        assert!(index < self.len());

        let raw_index = self.raw_index(index);
        self.raw.get(db, raw_index).expect("Invalid database")
            .end()
            .expect("Invalid database")
    }

    /// Set value at index.
    pub fn set(&mut self, db: &mut DB, index: usize, value: EndOf<DB>) {
        assert!(index < self.len());

        let raw_index = self.raw_index(index);
        self.raw.set(db, raw_index, Value::End(value));
    }

    /// Push a new value to the vector.
    pub fn push(&mut self, db: &mut DB, value: EndOf<DB>) {
        let old_len = self.len();
        if old_len == self.max_len() {
            self.extend(db);
        }
        let len = old_len + 1;
        let index = old_len;
        self.len = len;

        let raw_index = self.raw_index(index);
        self.raw.set(db, raw_index, Value::End(value));
    }

    /// Pop a value from the vector.
    pub fn pop(&mut self, db: &mut DB) -> Option<EndOf<DB>> {
        let old_len = self.len();
        if old_len == 0 {
            return None
        }

        let len = old_len - 1;
        let index = old_len - 1;
        let raw_index = self.raw_index(index);
        let value = self.raw.get(db, raw_index).map(|value| value.end().expect("Invalid format"));

        if len <= self.max_len() / 2 {
            self.shrink(db);
        }
        self.len = len;
        value
    }

    /// Get the length of the tuple.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Root of the current merkle tuple.
    pub fn root(&self) -> ValueOf<DB> {
        self.raw.root()
    }

    /// Root of the owned empty merkle.
    pub fn empty_root(&self) -> ValueOf<DB> {
        self.empty.root()
    }

    /// Drop the current tuple.
    pub fn drop(self, db: &mut DB) {
        self.raw.drop(db);
        self.empty.drop(db);
    }
}

impl<R: RootStatus, DB: MerkleDB> Leak for MerkleTuple<R, DB> {
    type Metadata = (ValueOf<DB>, ValueOf<DB>, usize);

    fn metadata(&self) -> Self::Metadata {
        let len = self.len();
        (self.raw.metadata(), self.empty.metadata(), len)
    }

    fn from_leaked((raw_root, empty_root, len): Self::Metadata) -> Self {
        Self {
            raw: MerkleRaw::from_leaked(raw_root),
            empty: MerkleEmpty::from_leaked(empty_root),
            len,
        }
    }
}

impl<DB: MerkleDB> MerkleTuple<OwnedRoot, DB> {
    /// Create a new tuple.
    pub fn create(db: &mut DB, len: usize) -> Self {
        let mut raw = MerkleEmpty::<OwnedRoot, DB>::default();
        let mut empty = MerkleEmpty::<OwnedRoot, DB>::default();

        let mut max_len = 1;
        while max_len < len {
            empty.extend(db);
            raw.extend(db);
            max_len *= 2;
        }

        let root = raw.metadata();

        Self {
            raw: MerkleRaw::<OwnedRoot, DB>::from_leaked(root),
            empty,
            len,
        }
    }
}
