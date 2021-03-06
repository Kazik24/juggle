//! Couple of helpful utilities.
//!
//! This module contains stuff useful for:
//! * Dividing CPU usage between multiple tasks ([`LoadBalance`](struct.LoadBalance.html)).
//! * Sharing value between threads without using locks ([`AtomicCell`](struct.AtomicCell.html)).
//! * Creating `Waker`s ([`to_waker`](fn.to_waker.html), [`func_waker`](fn.func_waker.html),
//! [`noop_waker`](fn.noop_waker.html)).



use alloc::sync::Arc;
use core::mem;
use core::ptr::null;
use core::task::{RawWaker, RawWakerVTable, Waker};
use core::future::Future;
use core::marker::PhantomData;
use core::ops::Deref;

mod cell;
mod load;
mod timing;
mod chunk_slab;
mod signal;
mod ucw;

pub use cell::AtomicCell;
pub use load::LoadBalance;
pub use timing::{TimerClock, TimerCount, TimingGroup};
#[cfg(feature = "std")]
pub use timing::StdTimerClock;

pub(crate) use chunk_slab::ChunkSlab;
pub(crate) use ucw::Ucw;


/// Implement this trait if you want to create custom waker with [`to_waker`](fn.to_waker.html) function.
pub trait DynamicWake {
    /// Perform waking action.
    fn wake(&self);
}

impl<F> DynamicWake for F where F: Fn() + Send + Sync + 'static {
    fn wake(&self) { self(); }
}

/// Convert atomic reference counted pointer to type implementing [`DynamicWake`](trait.DynamicWake.html)
/// into Waker.
///
/// Returned waker wraps given Arc so that cloning the waker will result also in cloning underlined
/// Arc. Invoking Waker's `wake` or `wake_by_ref` will call [`wake`](trait.DynamicWake.html#tymethod.wake)
/// on passed type implementing trait.
pub fn to_waker<T: DynamicWake + Send + Sync + 'static>(ptr: Arc<T>) -> Waker {
    //SAFETY: waker contract upheld.
    unsafe { pointer_as_dynamic_waker(Arc::into_raw(ptr)) }
}

/// Returns waker that performs no action when waked.
///
/// Returned waker can be used as dummy waker when polling future. This waker is static so using
/// `mem::forget` on it won't cause a leak.
pub fn noop_waker() -> Waker {
    fn clone_func(_: *const ()) -> RawWaker { RawWaker::new(null(), &TABLE) }
    static TABLE: RawWakerVTable = RawWakerVTable::new(clone_func, dummy, dummy, dummy);
    //SAFETY: waker contract upheld.
    unsafe { Waker::from_raw(clone_func(null())) }
}

fn dummy(_: *const ()) {}

/// Returns waker that performs action from specified function pointer when waked.
///
/// Returned waker can be supplied to [`block_on`](../fn.block_on.html) function to perform some
/// action when waked.
pub fn func_waker(func_ptr: fn()) -> Waker {
    if mem::size_of::<*const ()>() < mem::size_of::<fn()>() {
        // SAFETY: replacement logic in highly unlikely case where function pointers are longer than
        // data pointers, only available on very exotic platform configurations, should be ditched
        // by compiler on regular platforms
        return to_waker(Arc::new(func_ptr)); //here requires allocation
    }
    unsafe fn wake_func(ptr: *const ()) {
        //SAFETY: we know this void pointer points to some function with desired signature.
        let func = mem::transmute_copy::<*const (), fn()>(&ptr); //only way to cast
        func();
    }
    fn clone_func(ptr: *const ()) -> RawWaker { RawWaker::new(ptr, &TABLE) }
    //dummy drop impl, as function pointers doesn't need to be dropped.
    static TABLE: RawWakerVTable = RawWakerVTable::new(clone_func, wake_func, wake_func, dummy);
    //SAFETY: cast function to void pointer, safe to create waker.
    //VTABLE is constant, using clone_func reduces chance of making 2 copies in static memory
    unsafe { Waker::from_raw(clone_func(func_ptr as *const ())) }
}

/// Convert static reference to type implementing [`DynamicWake`](trait.DynamicWake.html)
/// into Waker.
///
/// Returned waker wraps given static reference and is safe to `mem::forget` without leak.
/// This method also doesn't perform any allocations.
pub fn to_static_waker<T: DynamicWake + Sync + 'static>(wake: &'static T)->Waker{
    struct Helper<T>(T);
    impl<T: DynamicWake + Sync + 'static> Helper<T> {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            Self::waker_clone,
            Self::waker_wake,
            Self::waker_wake,
            dummy,
        );
        //SAFETY: ptr is always &'static T
        unsafe fn waker_clone(ptr: *const ()) -> RawWaker {
            RawWaker::new(ptr, &Self::VTABLE)
        }
        //SAFETY: ptr is always &'static T
        unsafe fn waker_wake(ptr: *const ()) {
            (*(ptr as *const T)).wake();
        }
    }
    //VTABLE is constant, using waker_clone reduces chance of making 2 copies in static memory
    unsafe { Waker::from_raw(Helper::<T>::waker_clone(wake as *const T as *const ())) }
}

/// Struct for borrowing atomic reference counted pointer to type implementing [`DynamicWake`]
/// and treating it as Waker.
///
/// This struct implements `Deref<Target = Waker>`. Reference to waker supplied by this struct behaves exactly as
/// constructed by [`to_waker`] and can be cloned as usual. This struct is useful when you want temporary
/// waker but you're not a fan of the overhead with creating and destroying it.
#[repr(transparent)]
pub struct BorrowedWaker<'a>{
    inner: mem::ManuallyDrop<Waker>,
    _phantom: PhantomData<&'a Waker>,
}
impl<'a> BorrowedWaker<'a>{
    /// Construct [`BorrowedWaker`] by borrowing atomic reference counted pointer to type
    /// implementing [`DynamicWake`].
    ///
    /// To access `Waker` you only need to `Deref`erence this struct or coerce it to `&Waker`.
    pub fn new<T: DynamicWake + Send + Sync + 'static>(ptr: &'a Arc<T>)->Self{
        Self{
            //SAFETY: waker contract upheld.
            //Waker is wrapped inside BorrowedWaker preventing it from being moved out and dropped.
            inner: unsafe { mem::ManuallyDrop::new(pointer_as_dynamic_waker(Arc::as_ptr(ptr))) },
            _phantom: PhantomData
        }
    }
}
impl Deref for BorrowedWaker<'_>{
    type Target = Waker;
    fn deref(&self) -> &Self::Target { &self.inner }
}

unsafe fn pointer_as_dynamic_waker<T: DynamicWake + Send + Sync + 'static>(data: *const T)->Waker{
    struct Helper<T>(T);

    impl<T: DynamicWake + Send + Sync + 'static> Helper<T> {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            Self::waker_clone,
            Self::waker_wake,
            Self::waker_wake_by_ref,
            Self::waker_drop,
        );
        //SAFETY: ptr always contains valid Arc<T>
        unsafe fn waker_clone(ptr: *const ()) -> RawWaker {
            let arc = mem::ManuallyDrop::new(Arc::from_raw(ptr as *const T));
            mem::forget(arc.clone());
            RawWaker::new(ptr, &Self::VTABLE)
        }
        //SAFETY: ptr always contains valid Arc<T>
        unsafe fn waker_wake(ptr: *const ()) {
            let arc = Arc::from_raw(ptr as *const T);
            arc.wake();
        }
        //SAFETY: ptr always contains valid Arc<T>
        unsafe fn waker_wake_by_ref(ptr: *const ()) {
            let arc = mem::ManuallyDrop::new(Arc::from_raw(ptr as *const T));
            arc.wake();
        }
        //SAFETY: ptr always contains valid Arc<T>
        unsafe fn waker_drop(ptr: *const ()) {
            mem::drop(Arc::from_raw(ptr as *const T));
        }
    }

    let vtable = &Helper::<T>::VTABLE;
    //SAFETY: waker contract upheld only if pointer points to alive Arc<T>.
    unsafe { Waker::from_raw(RawWaker::new(data as *const (), vtable)) }
}

pub(crate) struct AtomicWakerRegistry {
    inner: AtomicCell<Option<Waker>>,
}

impl AtomicWakerRegistry {
    pub const fn empty() -> Self { Self { inner: AtomicCell::new(None) } }
    pub fn register(&self, waker: &Waker) -> bool { self.inner.swap(Some(waker.clone())).is_none() }
    pub fn clear(&self) -> bool { self.inner.swap(None).is_some() }
    pub fn notify_wake(&self) -> bool {
        match self.inner.swap(None) {
            Some(w) => {
                w.wake();
                true
            }
            None => false,
        }
    }
}

pub async fn forever<F: Future>(future: F)->!{
    future.await;
    panic!("Future was supposed to run forever :/");
}

pub(crate) struct DropGuard<F: FnOnce()>{
    inner: mem::MaybeUninit<F>,
}
impl<F: FnOnce()> DropGuard<F>{
    pub fn new(func: F)->Self{
        Self{
            inner: mem::MaybeUninit::new(func),
        }
    }
}

impl<F: FnOnce()> Drop for DropGuard<F>{
    fn drop(&mut self) {
        //SAFETY: move func from container, this is save because 'inner' is never used afterwards and
        //it has MaybeUninit type so it won't run any drop code.
        unsafe{
            let func = self.inner.as_ptr().read();
            func() //call moved function and consume it
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;
    use core::sync::atomic::*;
    use core::sync::atomic::Ordering::*;
    use core::convert::identity;
    use super::*;


    const WAKE: usize = 1;
    const DROP: usize = 2;

    struct TestWake(pub Arc<AtomicUsize>);

    //because Send Sync
    impl DynamicWake for TestWake {
        fn wake(&self) { self.0.fetch_or(WAKE, SeqCst); }
    }

    impl Drop for TestWake {
        fn drop(&mut self) { self.0.fetch_or(DROP, SeqCst); }
    }

    #[test]
    fn test_waker_drop_clone() {
        let test = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(TestWake(test.clone()));
        let waker = to_waker(inner.clone());
        assert_eq!(Arc::strong_count(&inner), 2);
        assert_eq!(test.load(SeqCst), 0);
        let other = waker.clone();
        assert_eq!(Arc::strong_count(&inner), 3);
        drop(waker);
        assert_eq!(Arc::strong_count(&inner), 2);
        drop(other);
        assert_eq!(Arc::strong_count(&inner), 1);
        let waker = to_waker(inner);
        assert_eq!(test.load(SeqCst), 0);
        drop(waker);
        assert_eq!(test.load(SeqCst), DROP);
    }

    #[test]
    fn test_waker_wake() {
        let test = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(TestWake(test.clone()));
        let waker = to_waker(inner.clone());
        waker.wake_by_ref();
        assert_eq!(test.load(SeqCst), WAKE);
        assert_eq!(Arc::strong_count(&inner), 2);
        test.store(0, SeqCst);
        assert_eq!(test.load(SeqCst), 0);
        waker.wake();
        assert_eq!(test.load(SeqCst), WAKE);
        assert_eq!(Arc::strong_count(&inner), 1);
        test.store(0, SeqCst);
        let waker = to_waker(inner);
        waker.wake_by_ref();
        assert_eq!(test.load(SeqCst), WAKE);
        test.store(0, SeqCst);
        assert_eq!(test.load(SeqCst), 0);
        test.store(0, SeqCst);
        waker.wake();
        assert_eq!(test.load(SeqCst), WAKE | DROP);
    }

    #[test]
    fn test_func_waker() {
        static WAKE_COUNT: AtomicUsize = AtomicUsize::new(0);
        if WAKE_COUNT.compare_exchange(0, 1, Relaxed, Relaxed).unwrap_or_else(identity) != 0 {
            unreachable!("Test was invoked concurrently!?!?");
        }
        let waker = func_waker(|| {
            WAKE_COUNT.fetch_add(1, Relaxed);
        });
        waker.wake_by_ref();
        let cloned = waker.clone();
        assert_eq!(WAKE_COUNT.load(Relaxed), 2);
        waker.wake();
        assert_eq!(WAKE_COUNT.load(Relaxed), 3);
        cloned.wake_by_ref();
        assert_eq!(WAKE_COUNT.load(Relaxed), 4);
        cloned.wake();
        assert_eq!(WAKE_COUNT.load(Relaxed), 5);
        WAKE_COUNT.store(0, Relaxed);
        assert_eq!(WAKE_COUNT.load(Relaxed), 0);
    }
    #[test]
    fn test_static_waker() {
        struct StaticWake(AtomicUsize);
        impl DynamicWake for StaticWake{
            fn wake(&self) {
                self.0.fetch_add(1, Relaxed);
            }
        }
        static STATIC_WAKE: StaticWake = StaticWake(AtomicUsize::new(0));

        if STATIC_WAKE.0.compare_exchange(0, 1, Relaxed, Relaxed).unwrap_or_else(identity) != 0 {
            unreachable!("Test was invoked concurrently!?!?");
        }
        let waker = to_static_waker(&STATIC_WAKE);
        waker.wake_by_ref();
        let cloned = waker.clone();
        assert_eq!(STATIC_WAKE.0.load(Relaxed), 2);
        waker.wake();
        assert_eq!(STATIC_WAKE.0.load(Relaxed), 3);
        cloned.wake_by_ref();
        assert_eq!(STATIC_WAKE.0.load(Relaxed), 4);
        cloned.wake();
        assert_eq!(STATIC_WAKE.0.load(Relaxed), 5);
        STATIC_WAKE.0.store(0, Relaxed);
        assert_eq!(STATIC_WAKE.0.load(Relaxed), 0);
    }

    #[test]
    fn test_borrowed_wake() {
        let test = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(TestWake(test.clone()));
        let waker_ref = BorrowedWaker::new(&inner);
        let waker = &*waker_ref;
        waker.wake_by_ref();
        assert_eq!(test.load(SeqCst), WAKE);
        assert_eq!(Arc::strong_count(&inner), 1);
        test.store(0, SeqCst);
        assert_eq!(test.load(SeqCst), 0);
        let waker2 = waker.clone();
        assert_eq!(test.load(SeqCst), 0);
        assert_eq!(Arc::strong_count(&inner), 2);
        waker2.wake();
        assert_eq!(test.load(SeqCst), WAKE);
        assert_eq!(Arc::strong_count(&inner), 1);
        test.store(0, SeqCst);
        waker.wake_by_ref();
        drop(inner);
        assert_eq!(test.load(SeqCst), WAKE | DROP);
    }
}