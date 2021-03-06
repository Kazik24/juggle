use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Helper struct for dealing with task switching.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct Yield(bool);

#[doc(hidden)]
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct YieldWhile<F: FnMut() -> bool>(F);

#[doc(hidden)]
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct YieldTimes { pub remaining: usize }

impl Yield {
    /// When awaited yields this task once. Causes task switch.
    ///
    /// For more convenient method of switching tasks see [`yield_once!()`](macro.yield_once.html) macro.
    ///
    /// When resulting Future is polled, for the first time it notifies the waker and returns
    /// `Poll::Pending`, second and all other polls return `Poll::Ready(())`.
    pub fn once() -> Self { Self(false) }

    /// When awaited it won't cause task switch.
    ///
    /// Future returned by this method when polled always return `Poll::Ready(())`.
    pub fn none() -> Self { Self(true) }

    /// When awaited yields this task specific number of times.
    ///
    /// Resulting Future notifies the waker and returns
    /// `Poll::Pending` 'remaining' number of times, all other polls return `Poll::Ready(())`.
    pub fn times(remaining: usize) -> YieldTimes { YieldTimes { remaining } }

    /// When awaited yields this task until provided closure returns false.
    ///
    /// Note that when first call on closure returns false, this task will not be yielded.
    /// This method is usefull when we want to do busy wait but also leave cpu time for
    /// other tasks.
    /// # Examples
    /// ```
    /// # fn main(){
    /// # use juggle::Yield;
    /// # use core::sync::atomic::{AtomicBool, Ordering};
    /// # smol::block_on(async move{
    /// let interrupt_flag: &AtomicBool = //...
    /// # &AtomicBool::new(true);
    ///
    /// Yield::yield_while(||!interrupt_flag.load(Ordering::Acquire)).await;
    /// # });
    /// # }
    /// ```
    pub fn yield_while<F>(predicate: F) -> YieldWhile<F> where F: FnMut() -> bool {
        YieldWhile(predicate)
    }
}

impl Future for Yield {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 { Poll::Ready(()) } else {
            self.get_mut().0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

impl<F: FnMut() -> bool> Future for YieldWhile<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //SAFETY: F does not care about being pinned
        let func = unsafe{ &mut self.get_unchecked_mut().0 };
        if !func() { Poll::Ready(()) } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

impl Future for YieldTimes {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.remaining == 0 { Poll::Ready(()) } else {
            self.as_mut().remaining -= 1;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}