use std::sync::Arc;
use core::mem;
use core::task::{RawWakerVTable, RawWaker, Waker};
use core::sync::atomic::{AtomicPtr, Ordering};
use core::ptr::null_mut;


pub trait DynamicWake{
    fn wake(&self);
}
pub fn to_waker<T: DynamicWake + Send + Sync + 'static>(ptr: Arc<T>)->Waker{
    let data = Arc::into_raw(ptr) as *const ();
    let vtable = &Helper::<T>::VTABLE;
    unsafe{Waker::from_raw(RawWaker::new(data,vtable))}
}

struct Helper<T>(T);
impl<T: DynamicWake + Send + Sync + 'static> Helper<T>{
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::waker_clone,
        Self::waker_wake,
        Self::waker_wake_by_ref,
        Self::waker_drop
    );
    unsafe fn waker_clone(ptr: *const ())->RawWaker{
        let arc = mem::ManuallyDrop::new(Arc::from_raw(ptr as *const T));
        mem::forget(arc.clone());
        RawWaker::new(ptr,&Self::VTABLE)
    }
    unsafe fn waker_wake(ptr: *const ()) {
        let arc = Arc::from_raw(ptr as *const T);
        arc.wake();
    }
    unsafe fn waker_wake_by_ref(ptr: *const ()) {
        let arc = mem::ManuallyDrop::new(Arc::from_raw(ptr as *const T));
        arc.wake();
    }
    unsafe fn waker_drop(ptr: *const ()){
        mem::drop(Arc::from_raw(ptr as *const T));
    }
}

pub(crate) struct AtomicOptionBox<T>(AtomicPtr<T>);
impl<T> AtomicOptionBox<T>{
    fn to_pointer(value: Option<Box<T>>)->*mut T{
        match value {
            Some(v) => Box::into_raw(v),
            None => null_mut(),
        }
    }
    fn from_pointer(ptr: *mut T)->Option<Box<T>>{
        if ptr.is_null() { None } else { Some(unsafe{ Box::from_raw(ptr) }) }
    }
    pub fn new(value: Option<Box<T>>)->Self{ Self(AtomicPtr::new(Self::to_pointer(value))) }
    pub fn swap(&self,value: Option<Box<T>>)->Option<Box<T>>{
        Self::from_pointer(self.0.swap(Self::to_pointer(value),Ordering::AcqRel))
    }
    pub fn is_none(&self,)->bool{ self.0.load(Ordering::Relaxed).is_null() }
}
impl<T> Drop for AtomicOptionBox<T>{
    fn drop(&mut self) {
        drop(Self::from_pointer(self.0.load(Ordering::Relaxed)));
    }
}

pub struct AtomicWakerRegistry{
    inner: AtomicOptionBox<Waker>,
}
impl AtomicWakerRegistry{
    pub fn empty()->Self{Self{inner:AtomicOptionBox::new(None)}}
    pub fn register(&self,waker: Waker)->bool{ self.inner.swap(Some(Box::new(waker))).is_none() }
    pub fn clear(&self)->bool{ self.inner.swap(None).is_some() }
    pub fn is_empty(&self)->bool{ self.inner.is_none() }
    pub fn notify_wake(&self)->bool{
        match self.inner.swap(None) {
            Some(w) =>{ w.wake(); true}
            None => false,
        }
    }
}