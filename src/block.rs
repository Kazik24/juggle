use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{RawWakerVTable, RawWaker, Waker, Context, Poll};
use core::ptr::null;
use core::pin::Pin;
use crate::utils::noop_waker;

pub(crate) trait Parker{
    /// Parks this thread is there is no token available, else returns immediately
    fn park(&self);
    /// Makes token available.
    fn unpark(&self);
}

pub(crate) struct SpinParker(AtomicBool);

impl Parker for SpinParker{
    fn park(&self) {
        while !self.0.compare_and_swap(true,false,Ordering::Acquire) {}
    }
    fn unpark(&self) { self.0.store(true,Ordering::Release); }
}


/// Utility for busy blocking on future.
///
/// Usefull for creating main loop on embedded systems. This function simply polls given future until it is ready.
/// Waker used in polling is no-op, when future yields then it is immediately polled again.
/// # Examples
/// ```
/// use juggle::*;
///
/// let result = spin_block_on(async move{
///     Yield::times(10).await;
///     10
/// });
/// assert_eq!(result,10);
/// ```
/// ```
/// # #[macro_use]
/// use juggle::*;
/// # fn do_some_processing(){}
///
/// let wheel = Wheel::new();
/// # let id =
/// wheel.handle().spawn(SpawnParams::default(),async move {
///     loop{
///         do_some_processing();
///         yield_once!();
///     }
/// });
/// wheel.handle().spawn(SpawnParams::default(),async move {
///     // some other processing tasks
/// });
/// # let handle = wheel.handle().clone();
/// # wheel.handle().spawn(SpawnParams::default(),async move {
/// #     Yield::times(10).await;
/// #     handle.cancel(id.unwrap());
/// # });
///
/// spin_block_on(wheel);
/// ```
pub fn spin_block_on<F>(mut future: F)->F::Output where F:Future{
    let mut pinned = unsafe{ Pin::new_unchecked(&mut future) };
    let dummy_waker = noop_waker();
    let mut ctx = Context::from_waker(&dummy_waker);
    loop{
        match pinned.as_mut().poll(&mut ctx) {
            Poll::Ready(value) => break value,
            Poll::Pending => {}
        }
    }
}

