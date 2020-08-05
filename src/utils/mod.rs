//! Couple of helpful utilities.
//!
//! This module contains primitives useful for working with tasks and synchronization.



use alloc::sync::Arc;
use core::mem;
use core::task::{RawWakerVTable, RawWaker, Waker};


mod cell;
mod load;
mod timing;

pub use cell::AtomicCell;
#[cfg(feature = "std")]
pub use load::LoadBalance;
pub use timing::*;
use core::ptr::null;


/// Implement this trait if you want to create custom waker with [to_waker](fn.to_waker.html) function.
pub trait DynamicWake{
    /// Perform waking action.
    fn wake(&self);
}
/// Convert atomic reference counted pointer to type implementing [DynamicWake](trait.DynamicWake.html)
/// into Waker.
///
/// Returned waker wraps given Arc so that cloning the waker will result also in cloning underlined
/// Arc. Invoking Waker's `wake` or `wake_by_ref` will call [wake](trait.DynamicWake.html#tymethod.wake)
/// on passed type implementing trait.
pub fn to_waker<T: DynamicWake + Send + Sync + 'static>(ptr: Arc<T>)->Waker{
    let data = Arc::into_raw(ptr) as *const ();
    let vtable = &Helper::<T>::VTABLE;
    unsafe{Waker::from_raw(RawWaker::new(data,vtable))}
}

/// Returns waker that performs no action when waked.
///
/// Returned waker can be used as dummy waker when polling future. This waker is static so using
/// `mem::forget` on it won't cause a leak.
pub fn noop_waker()->Waker{
    unsafe{ Waker::from_raw(RawWaker::new(null(),&NOOP_WAKER_VTABLE)) }
}
fn noop_clone(_: *const ()) -> RawWaker{ RawWaker::new(null(),&NOOP_WAKER_VTABLE) }
fn noop_dummy(_: *const ()) {}
static NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone,noop_dummy,noop_dummy,noop_dummy);

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

pub(crate) struct AtomicWakerRegistry{
    inner: AtomicCell<Option<Waker>>,
}
impl AtomicWakerRegistry{
    pub fn empty()->Self{Self{inner:AtomicCell::new(None)}}
    pub fn register(&self,waker: Waker)->bool{ self.inner.swap(Some(waker)).is_none() }
    pub fn clear(&self)->bool{ self.inner.swap(None).is_some() }
    pub fn notify_wake(&self)->bool{
        match self.inner.swap(None) {
            Some(w) =>{ w.wake(); true}
            None => false,
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use std::sync::atomic::*;

    const WAKE: usize = 1;
    const DROP: usize = 2;
    struct TestWake(pub Arc<AtomicUsize>); //because Send Sync
    impl DynamicWake for TestWake{
        fn wake(&self) { self.0.fetch_or(WAKE,Ordering::SeqCst); }
    }
    impl Drop for TestWake{
        fn drop(&mut self) { self.0.fetch_or(DROP,Ordering::SeqCst); }
    }
    #[test]
    fn test_waker_drop_clone(){
        let test = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(TestWake(test.clone()));
        let waker = to_waker(inner.clone());
        assert_eq!(Arc::strong_count(&inner),2);
        assert_eq!(test.load(Ordering::SeqCst),0);
        let other = waker.clone();
        assert_eq!(Arc::strong_count(&inner),3);
        drop(waker);
        assert_eq!(Arc::strong_count(&inner),2);
        drop(other);
        assert_eq!(Arc::strong_count(&inner),1);
        let waker = to_waker(inner);
        assert_eq!(test.load(Ordering::SeqCst),0);
        drop(waker);
        assert_eq!(test.load(Ordering::SeqCst),DROP);
    }
    #[test]
    fn test_waker_wake(){
        let test = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(TestWake(test.clone()));
        let waker = to_waker(inner.clone());
        waker.wake_by_ref();
        assert_eq!(test.load(Ordering::SeqCst),WAKE);
        assert_eq!(Arc::strong_count(&inner),2);
        test.store(0,Ordering::SeqCst);
        assert_eq!(test.load(Ordering::SeqCst),0);
        waker.wake();
        assert_eq!(test.load(Ordering::SeqCst),WAKE);
        assert_eq!(Arc::strong_count(&inner),1);
        test.store(0,Ordering::SeqCst);
        let waker = to_waker(inner);
        waker.wake_by_ref();
        assert_eq!(test.load(Ordering::SeqCst),WAKE);
        test.store(0,Ordering::SeqCst);
        assert_eq!(test.load(Ordering::SeqCst),0);
        test.store(0,Ordering::SeqCst);
        waker.wake();
        assert_eq!(test.load(Ordering::SeqCst),WAKE | DROP);
    }
}