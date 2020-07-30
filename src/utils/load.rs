use core::future::Future;
use core::task::{Context, Poll};
use core::pin::Pin;
use alloc::rc::Rc;
use core::cell::RefCell;
use crate::utils::{TimerClock, TimingGroup};


struct GenericLoadBalance<F: Future,I: TimerClock>{
    index: usize,
    group: Rc<(RefCell<TimingGroup<I::Duration>>,I)>,
    future: F,
}


impl<F: Future,I: TimerClock> GenericLoadBalance<F,I>{
    pub fn new_with(prop: u16,future: F,clk: I)->Self{
        let mut group = TimingGroup::new();
        let key = group.add(prop);
        Self{
            index: key,
            group: Rc::new((RefCell::new(group),clk)),
            future
        }
    }
    pub fn add<G>(&mut self,prop: u16,future: G)->GenericLoadBalance<G,I> where G: Future{
        let index = self.group.0.borrow_mut().add(prop);
        GenericLoadBalance{
            index,
            group: self.group.clone(), //clone rc
            future,
        }
    }
}


impl<F: Future,I: TimerClock> Drop for GenericLoadBalance<F,I>{
    fn drop(&mut self) {
        self.group.0.borrow_mut().remove(self.index);
    }
}

impl<F: Future,I: TimerClock> Future for GenericLoadBalance<F,I>{
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        if !self.group.0.borrow().can_execute(self.index) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let start = self.group.1.start();
        let pin = unsafe{ Pin::new_unchecked(&mut self.as_mut().get_unchecked_mut().future) };
        let res = pin.poll(cx);
        let dur = self.group.1.stop(start);
        self.group.0.borrow_mut().update_duration(self.index,dur);

        return res;
    }
}


/// Helper for equally dividing time slots across multiple tasks.
///
/// This struct can be used as a wrapper for tasks to ensure they have more-less equal amount of time
/// per slot to execute.
///
/// Each task has number of time slots assigned to it. Time slots divides time destinated for this group
/// into parts for each task. Some task can therefore have assigned more time than the others, e.g if
/// you have 2 tasks, first with 1 time slot and second with 2, then let them run for 3 seconds, first
/// task will be running 1 second from this time and second task - 2 seconds.
#[cfg(feature = "std")]
pub struct LoadBalance<F: Future>{
    inner: GenericLoadBalance<F,crate::utils::StdTiming>,
}
#[cfg(feature = "std")]
impl<F: Future> LoadBalance<F>{
    /// Create new load balancing group with one future in it and given number of time slots assigned to it.
    pub fn with(prop: u16,future: F)->Self{
        Self{inner: GenericLoadBalance::new_with(prop,future,crate::utils::StdTiming::default())}
    }
    /// Add future to this load balancing group with given number of time slots assigned. Returned wrapper
    /// also belongs to the same group as this wrapper.
    pub fn add<G>(&mut self,prop: u16,future: G)->LoadBalance<G> where G: Future{
        LoadBalance{inner:self.inner.add(prop,future)}
    }
}
#[cfg(feature = "std")]
impl<F: Future> Future for LoadBalance<F>{
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        unsafe{Pin::new_unchecked(&mut self.get_unchecked_mut().inner)}.poll(cx)
    }
}

