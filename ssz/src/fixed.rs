use bm::{ValueOf, Backend, Error, Value, DanglingPackedVector, DanglingVector, Leak, Sequence};
use bm::utils::vector_tree;
use primitive_types::{U256, H256};
use generic_array::GenericArray;

use crate::{IntoTree, FromTree, Intermediate, End, Composite};

pub trait IntoVectorTree<DB: Backend<Intermediate=Intermediate, End=End>> {
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>>;
}

pub trait FromVectorTree<DB: Backend<Intermediate=Intermediate, End=End>>: Sized {
    fn from_vector_tree(
        root: &ValueOf<DB>,
        db: &DB,
        len: usize,
        max_len: Option<usize>,
    ) -> Result<Self, Error<DB::Error>>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FixedVecRef<'a, T>(pub &'a [T]);
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FixedVec<T>(pub Vec<T>);

macro_rules! impl_builtin_fixed_uint_vector {
    ( $t:ty, $lt:ty ) => {
        impl<'a, DB> IntoVectorTree<DB> for FixedVecRef<'a, $t> where
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
                }).collect::<Vec<_>>(), db, max_len)
            }
        }

        impl<DB> FromVectorTree<DB> for FixedVec<$t> where
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

impl<'a, DB> IntoVectorTree<DB> for FixedVecRef<'a, U256> where
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

impl<DB> FromVectorTree<DB> for FixedVec<U256> where
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

impl<'a, DB> IntoVectorTree<DB> for FixedVecRef<'a, bool> where
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

        FixedVecRef(&bytes).into_vector_tree(db, max_len)
    }
}

impl<DB> FromVectorTree<DB> for FixedVec<bool> where
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

        Ok(Self(ret))
    }
}

impl<'a, DB, T: Composite> IntoVectorTree<DB> for FixedVecRef<'a, T> where
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

impl<DB, T: Composite> FromVectorTree<DB> for FixedVec<T> where
    T: FromTree<DB>,
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
            ret.push(T::from_tree(&value, db)?);
        }

        Ok(Self(ret))
    }
}

impl<'a, DB, T> IntoTree<DB> for FixedVecRef<'a, T> where
    Self: IntoVectorTree<DB>,
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_tree(&self, db: &mut DB) -> Result<ValueOf<DB>, Error<DB::Error>> {
        self.into_vector_tree(db, None)
    }
}

impl<DB, T> IntoVectorTree<DB> for FixedVec<T> where
    for<'a> FixedVecRef<'a, T>: IntoVectorTree<DB>,
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_vector_tree(
        &self,
        db: &mut DB,
        max_len: Option<usize>
    ) -> Result<ValueOf<DB>, Error<DB::Error>> {
        FixedVecRef(&self.0).into_vector_tree(db, max_len)
    }
}

impl<DB, T> IntoTree<DB> for FixedVec<T> where
    Self: IntoVectorTree<DB>,
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_tree(&self, db: &mut DB) -> Result<ValueOf<DB>, Error<DB::Error>> {
        self.into_vector_tree(db, None)
    }
}

impl<DB> IntoTree<DB> for H256 where
    DB: Backend<Intermediate=Intermediate, End=End>
{
    fn into_tree(&self, db: &mut DB) -> Result<ValueOf<DB>, Error<DB::Error>> {
        FixedVecRef(&self.0.as_ref()).into_tree(db)
    }
}
