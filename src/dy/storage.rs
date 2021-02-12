use std::sync::atomic::*;
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};

pub struct OwnedCell<T>{
    borrow: AtomicBool,
    cell: UnsafeCell<T>,
}

pub struct Ref<'a,T>{
    borrow: &'a AtomicBool,
    reference: &'a mut T,
}
unsafe impl<T> Send for OwnedCell<T> where T: Send {}
unsafe impl<T> Sync for OwnedCell<T> where T: Sync {}

impl<T> OwnedCell<T>{
    pub const fn new(value: T)->Self{
        Self{
            borrow: AtomicBool::new(false),
            cell: UnsafeCell::new(value),
        }
    }
    pub fn try_own(&self)->Result<Ref<T>,()>{
        if self.borrow.compare_and_swap(false,true,Ordering::AcqRel) {
            Err(())
        }else{
            Ok(Ref{
                borrow: &self.borrow,
                reference: unsafe{ &mut *self.cell.get() }
            })
        }
    }
    pub fn own(&self)->Ref<T>{ self.try_own().unwrap() }
    pub fn get_mut(&mut self)->&mut T{
        unsafe{ &mut *self.cell.get() }
    }
    pub fn into_inner(self)->T{ self.cell.into_inner() }
}


impl<'a,T> Drop for Ref<'a,T>{
    fn drop(&mut self) {
        self.borrow.store(false,Ordering::Release);
    }
}
impl<'a,T> Deref for Ref<'a,T>{
    type Target = T;
    fn deref(&self) -> &Self::Target { self.reference }
}
impl<'a,T> DerefMut for Ref<'a,T>{
    fn deref_mut(&mut self) -> &mut Self::Target { self.reference }
}