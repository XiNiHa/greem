//! The containers a resolver may return at a list position. One `Completes`
//! impl for `List<Ty>` covers all of them; `Items` is the way in for any
//! other collection.

use crate::resolver::Items;
use std::borrow::Borrow;
use std::sync::Arc;

/// A list output's items, consumed for completion and borrowed for
/// `Completes::first_error`.
pub trait ListOutput {
    type Item;

    fn into_items(self) -> impl Iterator<Item = Self::Item>;

    fn items(&self) -> impl Iterator<Item = impl Borrow<Self::Item>>;
}

impl<T> ListOutput for Vec<T> {
    type Item = T;

    fn into_items(self) -> impl Iterator<Item = T> {
        self.into_iter()
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<T>> {
        self.iter()
    }
}

impl<T> ListOutput for Box<[T]> {
    type Item = T;

    fn into_items(self) -> impl Iterator<Item = T> {
        self.into_vec().into_iter()
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<T>> {
        self.iter()
    }
}

/// Items cannot move out of a shared slice, so each one is cloned.
impl<T: Clone> ListOutput for Arc<[T]> {
    type Item = T;

    fn into_items(self) -> impl Iterator<Item = T> {
        (0..self.len()).map(move |i| self[i].clone())
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<T>> {
        self.iter()
    }
}

macro_rules! borrowed_slice {
    ($($container:ty),*) => {$(
        impl<'x, T> ListOutput for &'x $container {
            type Item = &'x T;

            fn into_items(self) -> impl Iterator<Item = &'x T> {
                self.iter()
            }

            fn items(&self) -> impl Iterator<Item = impl Borrow<&'x T>> {
                (*self).iter()
            }
        }
    )*};
}
borrowed_slice!([T], Vec<T>, Box<[T]>, Arc<[T]>);

impl<I> ListOutput for Items<I>
where
    I: IntoIterator,
    for<'r> &'r I: IntoIterator<Item: Borrow<I::Item>>,
{
    type Item = I::Item;

    fn into_items(self) -> impl Iterator<Item = I::Item> {
        self.0.into_iter()
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<I::Item>> {
        (&self.0).into_iter()
    }
}
