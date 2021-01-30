mod dyn_future;
mod algorithm;
mod wheel;
mod handle;
mod registry;
mod storage;
mod stat;
mod unordered_algorithm;

pub use self::handle::{IdNum, SpawnParams, State, WheelHandle};
pub use self::wheel::{LockedWheel, SuspendError, Wheel};

use core::cell::*;
use core::ops::{Deref, DerefMut};
use crate::round::unordered_algorithm::UnorderedAlgorithm;

// main algorithm for scheduling in Wheel
pub(crate) type Algorithm<'a> = UnorderedAlgorithm<'a>;

/// UnsafeCell wrapper.
/// SAFETY: Intended only to use inside this crate.
/// Provides runtime borrow checking in debug mode and only wraps UnsafeCell without any
/// checks in release mode.
pub(crate) struct Ucw<T>{
    #[cfg(debug_assertions)]
    inner: RefCell<T>,
    #[cfg(not(debug_assertions))]
    inner: UnsafeCell<T>,
}

pub(crate) struct UcwRef<'a,T>{
    #[cfg(debug_assertions)]
    inner: Ref<'a,T>,
    #[cfg(not(debug_assertions))]
    inner: &'a T,
}
pub(crate) struct UcwRefMut<'a,T>{
    #[cfg(debug_assertions)]
    inner: RefMut<'a,T>,
    #[cfg(not(debug_assertions))]
    inner: &'a mut T,
}
impl<T> Deref for UcwRef<'_,T>{
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target { self.inner.deref() }
}
impl<T> Deref for UcwRefMut<'_,T>{
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target { self.inner.deref() }
}
impl<T> DerefMut for UcwRefMut<'_,T>{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target { self.inner.deref_mut() }
}

impl<T> Ucw<T>{
    #[inline(always)]
    pub(crate) fn new(value: T)->Self{
        Self{
            #[cfg(debug_assertions)]
            inner: RefCell::new(value),
            #[cfg(not(debug_assertions))]
            inner: UnsafeCell::new(value),
        }
    }
    #[inline(always)]
    pub(crate) fn borrow(&self)->UcwRef<'_,T>{
        UcwRef{
            #[cfg(debug_assertions)]
            inner: self.inner.borrow(),
            #[cfg(not(debug_assertions))]
            inner: unsafe{ &*self.inner.get() },
        }
    }
    #[inline(always)]
    pub(crate) fn borrow_mut(&self)->UcwRefMut<'_,T>{
        UcwRefMut{
            #[cfg(debug_assertions)]
            inner: self.inner.borrow_mut(),
            #[cfg(not(debug_assertions))]
            inner: unsafe{ &mut *self.inner.get() },
        }
    }
}


