use std::rc::Rc;
use std::cell::UnsafeCell;
use crate::round::algorithm::SchedulerAlgorithm;
use super::handle::*;
use core::future::Future;
use std::pin::Pin;
use core::task::*;



/// Single-thread async task scheduler with dynamic task spawning and cancelling.
///
/// This structure is useful when you want to divide single thread to share processing power among
/// multiple task without preemption.
/// Tasks are scheduled using round-robin algorithm without priority. Which means that all tasks
/// are not starved cause effectively they have all the same priority.
///
/// # Managing tasks
/// You can spawn/suspend/resume/cancel any task as long as you have it's key, and handle to this
/// scheduler. Handle can be obtained from this object using method `handle(&self)`
pub struct Wheel{
    ptr: Rc<UnsafeCell<SchedulerAlgorithm>>,
    handle: WheelHandle,
}
/// Same as Wheel except that it has fixed content and there is no way to control state of tasks
/// within it.
/// [see] Wheel
pub struct LockedWheel{
    alg: SchedulerAlgorithm,
}

impl Wheel{
    pub fn new()->Self{
        let ptr = Rc::new(UnsafeCell::new(SchedulerAlgorithm::new()));
        let handle = WheelHandle::new(Rc::downgrade(&ptr));
        Self{ptr,handle}
    }

    pub fn handle(&self)->&WheelHandle{&self.handle}

    pub fn lock(self)->LockedWheel{
        // no panic cause rc has always strong count of 1 (it can have strong count > 1 during calls
        // on handle, but if these calls return then it will be back to 1)
        let alg = Rc::try_unwrap(self.ptr).ok().unwrap().into_inner();
        LockedWheel{alg}
    }
}

impl LockedWheel{
    pub fn unlock(self)->Wheel{
        let ptr = Rc::new(UnsafeCell::new(self.alg));
        let handle = WheelHandle::new(Rc::downgrade(&ptr));
        Wheel{ptr,handle}
    }
}


impl Future for Wheel{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe{&mut *self.as_mut().ptr.get() }.poll_internal(cx)
    }
}

impl Future for LockedWheel{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_mut().alg.poll_internal(cx)
    }
}