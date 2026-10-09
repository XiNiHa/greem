//! The containers a resolver may return at a list position. One `Completes`
//! impl for `List<Ty>` covers all of them but owned `Arc<[T]>`, which is
//! kept and completed as a borrowed slice; `Items` is the way in for any
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

/// A borrow of a borrowed list completes like the borrow it points to.
impl<'x, L: ?Sized> ListOutput for &&'x L
where
    &'x L: ListOutput,
{
    type Item = <&'x L as ListOutput>::Item;

    fn into_items(self) -> impl Iterator<Item = Self::Item> {
        (*self).into_items()
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<Self::Item>> {
        ListOutput::items(*self)
    }
}

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

/// Borrowed, the collection completes whatever it yields by reference.
impl<'x, I> ListOutput for &'x Items<I>
where
    &'x I: IntoIterator,
{
    type Item = <&'x I as IntoIterator>::Item;

    fn into_items(self) -> impl Iterator<Item = Self::Item> {
        (&self.0).into_iter()
    }

    fn items(&self) -> impl Iterator<Item = impl Borrow<Self::Item>> {
        (*self).into_items()
    }
}
