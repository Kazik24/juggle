mod dyn_future;
mod algorithm;
mod wheel;
mod handle;
mod registry;

pub use self::handle::{IdNum, SpawnParams, State, WheelHandle};
pub use self::wheel::{LockedWheel, SuspendError, Wheel};

use core::cell::*;
use core::ops::{Deref, DerefMut};

//unsafe cell wrapper
pub(crate) struct Ucw<T>{
    inner: RefCell<T>,
}

pub(crate) struct UcwRef<'a,T>{
    pub(crate)inner: Ref<'a,T>,
}
pub(crate) struct UcwRefMut<'a,T>{
    inner: RefMut<'a,T>,
}
impl<T> Deref for UcwRef<'_,T>{
    type Target = T;
    fn deref(&self) -> &Self::Target { self.inner.deref() }
}
impl<T> Deref for UcwRefMut<'_,T>{
    type Target = T;
    fn deref(&self) -> &Self::Target { self.inner.deref() }
}
impl<T> DerefMut for UcwRefMut<'_,T>{
    fn deref_mut(&mut self) -> &mut Self::Target { self.inner.deref_mut() }
}

impl<T> Ucw<T>{
    pub(crate) fn new(value: T)->Self{ Self{inner:RefCell::new(value)} }
    pub(crate) fn borrow(&self)->UcwRef<T>{ UcwRef{ inner: self.inner.borrow() }}
    pub(crate) fn borrow_mut(&self)->UcwRefMut<T>{ UcwRefMut{ inner: self.inner.borrow_mut() }}
}


