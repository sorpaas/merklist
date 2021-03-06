use crate::traits::{ReadBackend, WriteBackend, Construct, RootStatus, Owned, Dangling, Leak, Error, Tree, Sequence};
use crate::raw::Raw;
use crate::index::Index;

const ROOT_INDEX: Index = Index::root();
const EXTEND_INDEX: Index = Index::root().left();
const EMPTY_INDEX: Index = Index::root().right();

/// `Vector` with owned root.
pub type OwnedVector<C> = Vector<Owned, C>;

/// `Vector` with dangling root.
pub type DanglingVector<C> = Vector<Dangling, C>;

/// Binary merkle tuple.
pub struct Vector<R: RootStatus, C: Construct> {
	raw: Raw<R, C>,
	max_len: Option<u64>,
	len: usize,
}

impl<R: RootStatus, C: Construct> Vector<R, C> {
	fn raw_index(&self, i: usize) -> Index {
		Index::from_depth(i, self.depth())
	}

	fn extend<DB: WriteBackend<Construct=C> + ?Sized>(
		&mut self,
		db: &mut DB
	) -> Result<(), Error<DB::Error>> {
		let root = self.root();
		let mut new_raw = Raw::default();
		let empty = C::empty_at(db, self.depth())?;
		new_raw.set(db, EXTEND_INDEX, root)?;
		new_raw.set(db, EMPTY_INDEX, empty)?;
		self.raw.set(db, ROOT_INDEX, Default::default())?;
		self.raw = new_raw;
		Ok(())
	}

	fn shrink<DB: WriteBackend<Construct=C> + ?Sized>(
		&mut self,
		db: &mut DB
	) -> Result<(), Error<DB::Error>> {
		match self.raw.get(db, EXTEND_INDEX)? {
			Some(extended_value) => { self.raw.set(db, ROOT_INDEX, extended_value)?; },
			None => { self.raw.set(db, ROOT_INDEX, Default::default())?; },
		}
		Ok(())
	}

	/// Current maximum length of the vector.
	pub fn current_max_len(&self) -> u64 {
		self.max_len.unwrap_or({
			let mut max_len = 1;
			while max_len < self.len as u64 {
				max_len *= 2;
			}
			max_len
		})
	}

	/// Overall maximum length of the vector.
	pub fn max_len(&self) -> Option<u64> {
		self.max_len
	}

	/// Depth of the vector.
	pub fn depth(&self) -> usize {
		let mut max_len = 1;
		let mut depth = 0;
		while max_len < self.current_max_len() {
			max_len *= 2;
			depth += 1;
		}
		depth
	}

	/// Get value at index.
	pub fn get<DB: ReadBackend<Construct=C> + ?Sized>(
		&self,
		db: &mut DB,
		index: usize
	) -> Result<C::Value, Error<DB::Error>> {
		if index >= self.len() {
			return Err(Error::AccessOverflowed)
		}

		let raw_index = self.raw_index(index);
		self.raw.get(db, raw_index)?.ok_or(Error::CorruptedDatabase)
	}

	/// Set value at index.
	pub fn set<DB: WriteBackend<Construct=C> + ?Sized>(
		&mut self,
		db: &mut DB,
		index: usize,
		value: C::Value
	) -> Result<(), Error<DB::Error>> {
		if index >= self.len() {
			return Err(Error::AccessOverflowed)
		}

		let raw_index = self.raw_index(index);
		self.raw.set(db, raw_index, value)?;
		Ok(())
	}

	/// Push a new value to the vector.
	pub fn push<DB: WriteBackend<Construct=C> + ?Sized>(
		&mut self,
		db: &mut DB,
		value: C::Value
	) -> Result<(), Error<DB::Error>> {
		let old_len = self.len();
		if (old_len as u64) == self.current_max_len() {
			if self.max_len.is_some() {
				return Err(Error::AccessOverflowed)
			} else {
				self.extend(db)?;
			}
		}
		let len = old_len + 1;
		let index = old_len;
		self.len = len;

		let raw_index = self.raw_index(index);
		self.raw.set(db, raw_index, value)?;
		Ok(())
	}

	/// Pop a value from the vector.
	pub fn pop<DB: WriteBackend<Construct=C> + ?Sized>(
		&mut self,
		db: &mut DB
	) -> Result<Option<C::Value>, Error<DB::Error>> {
		let old_len = self.len();
		if old_len == 0 {
			return Ok(None)
		}

		let len = old_len - 1;
		let index = old_len - 1;
		let raw_index = self.raw_index(index);
		let value = self.raw.get(db, raw_index)?.ok_or(Error::CorruptedDatabase)?;

		let mut empty_depth_to_bottom = 0;
		let mut replace_index = raw_index;
		loop {
			if let Some(parent) = replace_index.parent() {
				if parent.left() == replace_index {
					replace_index = parent;
					empty_depth_to_bottom += 1;
				} else {
					break
				}
			} else {
				break
			}
		}
		let empty = C::empty_at(db, empty_depth_to_bottom)?;
		self.raw.set(db, replace_index, empty)?;

		if (len as u64) <= self.current_max_len() / 2 {
			if self.max_len.is_none() {
				self.shrink(db)?;
			}
		}
		self.len = len;
		Ok(Some(value))
	}

	/// Get the length of the tuple.
	pub fn len(&self) -> usize {
		self.len
	}

	/// Create a tuple from raw merkle tree.
	pub fn from_raw(raw: Raw<R, C>, len: usize, max_len: Option<u64>) -> Self {
		Self { raw, len, max_len }
	}
}

impl<R: RootStatus, C: Construct> Tree for Vector<R, C> {
	type RootStatus = R;
	type Construct = C;

	fn root(&self) -> C::Value {
		self.raw.root()
	}

	fn drop<DB: WriteBackend<Construct=C> + ?Sized>(
		self,
		db: &mut DB
	) -> Result<(), Error<DB::Error>> {
		self.raw.drop(db)?;
		Ok(())
	}

	fn into_raw(self) -> Raw<R, C> {
		self.raw
	}
}

impl<R: RootStatus, C: Construct> Sequence for Vector<R, C> {
	fn len(&self) -> usize {
		self.len
	}
}

impl<R: RootStatus, C: Construct> Leak for Vector<R, C> {
	type Metadata = (C::Value, usize, Option<u64>);

	fn metadata(&self) -> Self::Metadata {
		let len = self.len();
		let max_len = self.max_len;
		(self.raw.metadata(), len, max_len)
	}

	fn from_leaked((raw_root, len, max_len): Self::Metadata) -> Self {
		Self {
			raw: Raw::from_leaked(raw_root),
			len,
			max_len,
		}
	}
}

impl<C: Construct> Vector<Owned, C> {
	/// Create a new tuple.
	pub fn create<DB: WriteBackend<Construct=C> + ?Sized>(
		db: &mut DB,
		len: usize,
		max_len: Option<u64>
	) -> Result<Self, Error<DB::Error>> {
		if let Some(max_len) = max_len {
			if (len as u64) < max_len || max_len == 0 {
				return Err(Error::InvalidParameter)
			}
		}

		let mut raw = Raw::<Owned, C>::default();

		let target_len = max_len.unwrap_or(len as u64);
		let mut current_max_len = 1;
		let mut depth = 0;
		while current_max_len < target_len {
			current_max_len *= 2;
			depth += 1;
		}

		let empty = C::empty_at(db, depth)?;
		raw.set(db, ROOT_INDEX, empty)?;

		Ok(Self {
			raw,
			len,
			max_len,
		})
	}
}

impl<R: RootStatus, C: Construct> Raw<R, C> {
	/// Convert the current value to a vector.
	pub fn into_vector(self, len: usize, max_len: Option<u64>) -> Vector<R, C> {
		Vector::from_raw(self, len, max_len)
	}
}
