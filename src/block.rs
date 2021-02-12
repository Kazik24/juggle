use core::future::Future;
use core::pin::Pin;
use core::hint::spin_loop;
use core::task::{Context, Poll, Waker};
use crate::utils::noop_waker;

/// Utility for busy blocking on future.
///
/// Usefull for creating main loop on embedded systems. This function simply polls given future until it is ready.
/// Waker used in polling is no-op, when future yields then it is polled again (with `spin_loop_hint` optimization).
///
/// Equivalent to `block_on(future, || spin_loop(), &noop_waker())`.
/// # Examples
/// ```
/// use juggle::*;
///
/// let result = spin_block_on(async move {
///     Yield::times(10).await;
///     10
/// });
/// assert_eq!(result,10);
/// ```
/// ```
/// # #[macro_use]
/// use juggle::{*, dy::*};
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
/// # let handle = wheel.handle().clone();
/// wheel.handle().spawn(SpawnParams::default(),async move {
///     // some other processing tasks
/// #   Yield::times(10).await;
/// #   handle.cancel(id.unwrap());
/// });
///
/// spin_block_on(wheel).unwrap();
/// ```
pub fn spin_block_on<F>(future: F) -> F::Output where F: Future {
    block_on(future, || spin_loop(), &noop_waker())
}

/// Utility for blocking on future with custom waker and pending action.
///
/// # Examples
/// ```
/// use juggle::*;
/// use juggle::utils::noop_waker;
///
/// let mut yield_count = 0;
/// let future = async move{
///     Yield::times(10).await;
///     20
/// };
///
/// let result = block_on(future,||{ yield_count +=1; },&noop_waker());
/// assert_eq!(result,20);
/// assert_eq!(yield_count,10);
/// ```
pub fn block_on<F>(mut future: F, mut on_pending: impl FnMut(), waker: &Waker) -> F::Output where F: Future {
    // SAFETY: we know this future is on stack and cannot be moved in other way
    // because this block shadows it, preventing from access.
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let mut ctx = Context::from_waker(waker);
    loop {
        match future.as_mut().poll(&mut ctx) {
            Poll::Ready(value) => break value,
            Poll::Pending => { on_pending(); }
        }
    }
}

