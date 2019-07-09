use bm::{ValueOf, Backend, Error, Value, DanglingPackedVector, DanglingVector, Leak, Sequence};
use bm::utils::{vector_tree, host_len};
use primitive_types::U256;
use generic_array::GenericArray;
use alloc::vec::Vec;

use crate::{IntoTree, FromTree, FromTreeWithConfig, Intermediate, End, Composite};

/// Implement FromVectorTreeWithConfig for traits that has already
/// implemented FromVectorTree and does not need extra configs.
#[macro_export]
macro_rules! impl_from_vector_tree_with_empty_config {
    ( $t:ty ) => {
        impl<C, DB> $crate::FromVectorTreeWithConfig<C, DB> for $t where
            DB: $crate::Backend<Intermediate=Intermediate, End=End>
        {
            fn from_vector_tree_with_config(
                root: &$crate::ValueOf<DB>,
                db: &DB,
                len: usize,
                max_len: Option<usize>,
                _config: &C,
            ) -> Result<Self, $crate::Error<DB::Error>> {
                <$t>::from_vector_tree(root, db, len, max_len)
            }
        }
    }
}

/// Traits for vector converting into a tree structure.
pub trait IntoVectorTree<DB: Backend<Intermediate=Intermediate, End=End>> {
    /// Convert this vector into merkle tree, writing nodes into the
    /// given database, and using the maximum length specified.
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>>;
}

/// Traits for vector converting from a tree structure.
pub trait FromVectorTree<DB: Backend<Intermediate=Intermediate, End=End>>: Sized {
    /// Convert this type from merkle tree, reading nodes from the
    /// given database, with given length and maximum length.
    fn from_vector_tree(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>,
    ) -> Result<Self, Error<DB::Error>>;
}

/// Traits for vector converting from a tree structure with config.
pub trait FromVectorTreeWithConfig<C, DB: Backend<Intermediate=Intermediate, End=End>>: Sized {
    /// Convert this type from merkle tree, reading nodes from the
    /// given database, with given length and maximum length.
    fn from_vector_tree_with_config(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>,
        config: &C,
    ) -> Result<Self, Error<DB::Error>>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// Elemental `Vec` reference. In ssz's definition, this is a basic "vector".
pub struct ElementalFixedVecRef<'a, T>(pub &'a [T]);
#[derive(Debug, Clone, Eq, PartialEq)]
/// Elemental `Vec` value. In ssz's definition, this is a basic "vector".
pub struct ElementalFixedVec<T>(pub Vec<T>);

macro_rules! impl_builtin_fixed_uint_vector {
    ( $t:ty, $lt:ty ) => {
        impl<'a, DB> IntoVectorTree<DB> for ElementalFixedVecRef<'a, $t> where
            DB: Backend<Intermediate=Intermediate, End=End>
        {
            fn into_vector_tree(
                &self,
                db: &mut DB,
                max_len: Option<usize>
            ) -> Result<ValueOf<DB>, Error<DB::Error>> {
                let mut chunks: Vec<Vec<u8>> = Vec::new();

                for value in self.0 {
                    if chunks.last().map(|v| v.len() == 32).unwrap_or(true) {
                        chunks.push(Vec::new());
                    }

                    let current = chunks.last_mut().expect("chunks must have at least one item; qed");
                    current.append(&mut value.to_le_bytes().into_iter().cloned().collect::<Vec<u8>>());
                }

                if let Some(last) = chunks.last_mut() {
                    while last.len() < 32 {
                        last.push(0u8);
                    }
                }

                vector_tree(&chunks.into_iter().map(|c| {
                    let mut ret = End::default();
                    ret.0.copy_from_slice(&c);
                    Value::End(ret)
                }).collect::<Vec<_>>(), db, max_len.map(|max| host_len::<typenum::U32, $lt>(max)))
            }
        }

        impl_from_vector_tree_with_empty_config!(ElementalFixedVec<$t>);
        impl<DB> FromVectorTree<DB> for ElementalFixedVec<$t> where
            DB: Backend<Intermediate=Intermediate, End=End>
        {
            fn from_vector_tree(
                root: &ValueOf<DB>,
                db: &DB,
                len: usize,
                max_len: Option<usize>
            ) -> Result<Self, Error<DB::Error>> {
                let packed = DanglingPackedVector::<DB, GenericArray<u8, $lt>, typenum::U32, $lt>::from_leaked(
                    (root.clone(), len, max_len)
                );

                let mut ret = Vec::new();
                for i in 0..len {
                    let value = packed.get(db, i)?;
                    let mut bytes = <$t>::default().to_le_bytes();
                    bytes.copy_from_slice(value.as_slice());
                    ret.push(<$t>::from_le_bytes(bytes));
                }

                Ok(Self(ret))
            }
        }
    }
}

impl_builtin_fixed_uint_vector!(u8, typenum::U1);
impl_builtin_fixed_uint_vector!(u16, typenum::U2);
impl_builtin_fixed_uint_vector!(u32, typenum::U4);
impl_builtin_fixed_uint_vector!(u64, typenum::U8);
impl_builtin_fixed_uint_vector!(u128, typenum::U16);

impl<'a, DB> IntoVectorTree<DB> for ElementalFixedVecRef<'a, U256> where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>> {
        vector_tree(&self.0.iter().map(|uint| {
            let mut ret = End::default();
            uint.to_little_endian(&mut ret.0);
            Value::End(ret)
        }).collect::<Vec<_>>(), db, max_len)
    }
}

impl_from_vector_tree_with_empty_config!(ElementalFixedVec<U256>);
impl<DB> FromVectorTree<DB> for ElementalFixedVec<U256> where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn from_vector_tree(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>
    ) -> Result<Self, Error<DB::Error>> {
        let vector = DanglingVector::<DB>::from_leaked(
            (root.clone(), len, max_len)
        );

        let mut ret = Vec::new();
        for i in 0..len {
            let value = vector.get(db, i)?;
            ret.push(U256::from(value.as_ref()));
        }

        Ok(Self(ret))
    }
}

impl<'a, DB> IntoVectorTree<DB> for ElementalFixedVecRef<'a, bool> where
    DB: Backend<Intermediate=Intermediate, End=End>,
{
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>> {
        let mut bytes = Vec::new();
        bytes.resize((self.0.len() + 7) / 8, 0u8);

        for i in 0..self.0.len() {
            bytes[i / 8] |= (self.0[i] as u8) << (i % 8);
        }

        ElementalFixedVecRef(&bytes).into_vector_tree(db, max_len)
    }
}

impl_from_vector_tree_with_empty_config!(ElementalFixedVec<bool>);
impl<DB> FromVectorTree<DB> for ElementalFixedVec<bool> where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn from_vector_tree(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>
    ) -> Result<Self, Error<DB::Error>> {
        let packed = DanglingPackedVector::<DB, GenericArray<u8, typenum::U1>, typenum::U32, typenum::U1>::from_leaked(
            (root.clone(), (len + 7) / 8, max_len.map(|l| (l + 7) / 8))
        );

        let mut bytes = Vec::new();
        for i in 0..packed.len() {
            bytes.push(packed.get(db, i)?[0]);
        }
        let mut ret = Vec::new();
        for i in 0..len {
            ret.push(bytes[i / 8] & (1 << (i % 8)) != 0);
        }
        // TODO: check to make sure rest of the bits are unset.

        Ok(Self(ret))
    }
}

impl<'a, DB, T: Composite> IntoVectorTree<DB> for ElementalFixedVecRef<'a, T> where
    T: IntoTree<DB>,
    DB: Backend<Intermediate=Intermediate, End=End>,
{
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>> {
        vector_tree(&self.0.iter().map(|value| {
            value.into_tree(db)
        }).collect::<Result<Vec<_>, _>>()?, db, max_len)
    }
}

fn from_vector_tree<T, F, DB>(
    root: &ValueOf<DB>,
    db: &DB,
    len: usize,
    max_len: Option<usize>,
    f: F
) -> Result<ElementalFixedVec<T>, Error<DB::Error>> where
    DB: Backend<Intermediate=Intermediate, End=End>,
    F: Fn(&ValueOf<DB>, &DB) -> Result<T, Error<DB::Error>>
{
    let vector = DanglingVector::<DB>::from_leaked(
        (root.clone(), len, max_len)
    );
    let mut ret = Vec::new();

    for i in 0..len {
        let value = vector.get(db, i)?;
        ret.push(f(&value, db)?);
    }

    Ok(ElementalFixedVec(ret))
}

impl<DB, T: Composite + FromTree<DB>> FromVectorTree<DB> for ElementalFixedVec<T> where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn from_vector_tree(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>
    ) -> Result<Self, Error<DB::Error>> {
        from_vector_tree(root, db, len, max_len, |value, db| T::from_tree(value, db))
    }
}

impl<DB, C, T: Composite + FromTreeWithConfig<C, DB>> FromVectorTreeWithConfig<C, DB> for ElementalFixedVec<T> where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn from_vector_tree_with_config(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>,
        config: &C,
    ) -> Result<Self, Error<DB::Error>> {
        from_vector_tree(root, db, len, max_len, |value, db| {
            T::from_tree_with_config(value, db, config)
        })
    }
}

impl<DB, T> IntoVectorTree<DB> for ElementalFixedVec<T> where
    for<'a> ElementalFixedVecRef<'a, T>: IntoVectorTree<DB>,
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>> {
        ElementalFixedVecRef(&self.0).into_vector_tree(db, max_len)
    }
}