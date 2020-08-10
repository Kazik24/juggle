use core::future::Future;
use core::task::{Context, Poll};
use core::pin::Pin;
use crate::utils::noop_waker;


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
/// # let handle = wheel.handle().clone();
/// wheel.handle().spawn(SpawnParams::default(),async move {
///     // some other processing tasks
/// #   Yield::times(10).await;
/// #   handle.cancel(id.unwrap());
/// });
///
/// spin_block_on(wheel).unwrap();
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

