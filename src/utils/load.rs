use core::future::Future;
use core::task::{Context, Poll};
use core::pin::Pin;
use alloc::rc::Rc;
use core::cell::RefCell;
use crate::utils::{TimerClock, TimingGroup};
use core::fmt::{Debug, Formatter};


/// Helper for equally dividing time slots across multiple tasks.
///
/// This struct can be used as a wrapper for tasks to ensure they have more-less equal amount of time
/// per slot to execute. For more low-level control you can use [TimingGroup](struct.TimingGroup.html).
///
/// Each task has number of time slots assigned to it. Time slots divides time designated for this group
/// into parts for each task. Some task can therefore have assigned more time than the others, e.g if
/// you have 2 tasks, first with 1 time slot and second with 2, then let them run for 3 seconds, first
/// task will be running 1 second from this time and second task - 2 seconds.
///
/// You can specify custom clock for measuring time in this group by implementing
/// [TimerClock](trait.TimerClock.html) trait and passing it to [with](#method.with) method.
pub struct LoadBalance<F: Future,C: TimerClock>{
    record: Registered<C>,
    future: F,
}
struct Registered<C: TimerClock>{
    index: usize,
    group: Rc<(RefCell<TimingGroup<C::Duration>>,C)>,
}
impl<C: TimerClock> Registered<C>{
    fn unregister_and_get(self)->Rc<(RefCell<TimingGroup<C::Duration>>,C)>{
        //equivalent to running drop
        self.group.0.borrow_mut().remove(self.index);
        unsafe{
            let rc = core::ptr::read(&self.group as *const _);//take by force!
            core::mem::forget(self); //you'd better not drop it!
            rc
        }
    }
}
impl<C: TimerClock> Drop for Registered<C>{
    fn drop(&mut self) {
        self.group.0.borrow_mut().remove(self.index);
    }
}
impl<F: Future,C: TimerClock> LoadBalance<F,C>{
    /// Create new load balancing group with one future in it. Assigns specific
    /// number of time slots to future and clock used as measuring time source for this group.
    /// For usage with `std` library feature, you can use [StdTimerClock](struct.StdTimerClock.html)
    /// as measuring time source.
    pub fn with(clock: C,prop: u16,future: F)->Self{
        let mut group = TimingGroup::new();
        let key = group.insert(prop);
        Self{
            record: Registered{
                index: key,
                group: Rc::new((RefCell::new(group),clock)),
            },
            future
        }
    }
    /// Insert future to this load balancing group with given number of time slots assigned. Returned wrapper
    /// also belongs to the same group as this wrapper.
    pub fn insert<G>(&self, prop: u16, future: G) ->LoadBalance<G,C> where G: Future{
        let index = self.record.group.0.borrow_mut().insert(prop);
        LoadBalance{
            record: Registered{
                index,group: self.record.group.clone(),//clone rc
            },
            future,
        }
    }
    /// Returns number of tasks registered in this group.
    pub fn count(&self)->usize{ self.record.group.0.borrow().count() }

    /// Remove this wrapper from load balancing group, if this task is last then `Some` is returned
    /// with ownership of clock. Otherwise returns `None`.
    pub fn remove(self)->Option<C>{
        let group = self.record.unregister_and_get();
        Rc::try_unwrap(group).ok().map(|p|p.1)
    }
}
impl<F: Future,C: TimerClock> Future for LoadBalance<F,C>{
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        if !self.record.group.0.borrow().can_execute(self.record.index) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let start = self.record.group.1.start(); //start measure
        let res = unsafe{
            let pin = Pin::new_unchecked(&mut self.as_mut().get_unchecked_mut().future);
            pin.poll(cx)
        };
        let dur = self.record.group.1.stop(start); //end measure
        self.record.group.0.borrow_mut().update_duration(self.record.index,dur);

        return res;
    }
}

impl<F: Future + Debug,C: TimerClock + Debug> Debug for LoadBalance<F,C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LoadBalance").field("future",&self.future) //pinned future by shared ref
            .field("slots",&self.record.group.0.borrow().get_slot_count(self.record.index).unwrap())
            .field("clock",&self.record.group.1).finish()
    }
}

