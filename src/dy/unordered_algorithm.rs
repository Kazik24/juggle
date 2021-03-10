use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::fmt::Result;
use core::mem::swap;
use core::iter::from_fn;
use core::task::{Context, Poll, Waker};
use crate::dy::dyn_future::{DynamicFuture, TaskName};
use crate::dy::handle::State;
use crate::dy::registry::Registry;
use crate::utils::{AtomicWakerRegistry, DropGuard, Ucw};
use crate::dy::stat::{TaskWrapper, StopReason, TaskRegistry};
use core::cmp::max;

pub(crate) type TaskKey = usize;


pub(crate) struct UnorderedAlgorithm<R: TaskRegistry<TaskKey>> where R::Task: TaskWrapper {
    registry: R,
    last_waker: Arc<AtomicWakerRegistry>,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
}
#[repr(u8)]
enum Rotate { Wait, Continue }

impl<R: TaskRegistry<TaskKey> + Default> UnorderedAlgorithm<R> where R::Task: TaskWrapper {
    pub(crate) fn new() -> Self { Self::with(R::default()) }
}
impl<R: TaskRegistry<TaskKey>> UnorderedAlgorithm<R> where R::Task: TaskWrapper {
    pub(crate) fn with(registry: R) -> Self {
        Self {
            registry,
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            suspended_count: Cell::new(0),
            current: Cell::new(None),
        }
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.current.get() }
    pub(crate) fn clone_registry(&self) -> Arc<AtomicWakerRegistry> { self.last_waker.clone() }
    fn inc_suspended(&self) { self.suspended_count.set(self.suspended_count.get() + 1) }
    fn dec_suspended(&self) { self.suspended_count.set(self.suspended_count.get() - 1) }
    pub(crate) fn register(&self, dynamic: R::Task) -> Option<TaskKey> {
        let suspended = dynamic.get_stop_reason() == StopReason::Suspended;
        let key = self.registry.insert(dynamic); //won't realloc other futures because it uses ChunkSlab
        if suspended && key.is_some() {
            //increase count cause added task was suspended
            self.inc_suspended();
        }
        key
    }

    //safe to call from inside task
    pub(crate) fn resume(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) if task.get_stop_reason() == StopReason::Suspended => {
                task.set_stop_reason(StopReason::None);
                self.dec_suspended();
                true
            }
            _ => false,
        }
    }

    //if rotate_once encounters suspended task, then it will be removed from queue
    pub(crate) fn suspend(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) if task.get_stop_reason() == StopReason::None => {
                task.set_stop_reason(StopReason::Suspended);
                self.inc_suspended();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_state(&self, key: TaskKey) -> State {
        match self.registry.get(key) {
            Some(task) => match task.get_stop_reason() {
                StopReason::Cancelled => State::Cancelled,
                StopReason::Suspended => State::Suspended,
                _ => {
                    if task.is_runnable() { State::Runnable }
                    else { State::Waiting }
                }
            }
            None => State::Inactive,
        }
    }

    //if rotate_once encounters cancelled task, then it will be removed from queue and registry
    pub(crate) fn cancel(&self, key: TaskKey) -> bool {
        if let Some(task) = self.registry.get(key) {
            let r = task.get_stop_reason();
            if r != StopReason::Cancelled {
                task.set_stop_reason(StopReason::Cancelled);
                if r == StopReason::Suspended {
                    self.dec_suspended();
                }
                return true;
            }
        }
        false
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey> {
        for k in 0..self.registry.capacity() {
            match self.registry.get(k) {
                Some(v) => match v.get_name().as_str() {
                    Some(n) if n == name => return Some(k),
                    _ => {}
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn registered_count(&self)->usize{ self.registry.count() }

    pub fn with_name<F, T>(&self, id: TaskKey, func: F) -> T where F: FnOnce(&TaskName) -> T {
        match self.registry.get(id) {
            Some(task) => func(task.get_name()),
            None => func(&TaskName::None),
        }
    }

    pub(crate) fn format_internal(&self, f: &mut Formatter<'_>, name: &str) -> Result {
        pub(crate) struct DebugTask<'a,R>(
            &'a R,
            Option<TaskKey>,
        );

        impl<'a,R: TaskRegistry<TaskKey>> Debug for DebugTask<'a,R> where R::Task: TaskWrapper {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self.1 {
                    Some(id) => {
                        if let Some(task) = self.0.get(id) {
                            return match task.get_name().as_str() {
                                Some(s) => write!(f, "0x{:X}:{}", id, s),
                                None => write!(f, "0x{:X}", id),
                            };
                        }
                    }
                    _ => {}
                }
                write!(f, "None")
            }
        }

        writeln!(f, "{}{{", name)?;
        let span = 10;
        writeln!(f, "{:>s$}: {:?}", "current", DebugTask(&self.registry, self.current.get()), s = span)?;

        // struct RunnableDebug<'a, 'b>(&'a UnorderedAlgorithm<'b>);
        // impl<'a, 'b> Debug for RunnableDebug<'a, 'b> {
        //     fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        //         if self.0.runnable.0.borrow().is_empty() && self.0.runnable.1.borrow().is_empty() { write!(f, "None") } else {
        //
        //             let mut buff0 = &self.0.runnable.0.borrow();
        //             let mut buff1 = &self.0.runnable.1.borrow();
        //             if self.0.which_buffer.get() { swap(&mut buff0, &mut buff1); }
        //             let buff0 = buff0.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
        //             let buff1 = buff1.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
        //             f.debug_list().entries(buff0).entries(buff1).finish()
        //         }
        //     }
        // }
        // writeln!(f, "{:>s$}: {:?}", "runnable", RunnableDebug(self), s = span)?;
        //
        // struct WaitingDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        // impl<'a, 'b> Debug for WaitingDebug<'a, 'b> {
        //     fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        //         let deferred = self.0.ctrl.deferred.borrow();
        //         let buff = deferred.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
        //         if deferred.is_empty() { write!(f, "None") } else {
        //             f.debug_list().entries(buff).finish()
        //         }
        //     }
        // }
        // writeln!(f, "{:>s$}: {:?}", "waiting", WaitingDebug(self), s = span)?;
        //
        // struct SuspendedDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        // impl<'a, 'b> Debug for SuspendedDebug<'a, 'b> {
        //     fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        //         let mut buff = self.0.ctrl.registry.iter().map(|(k, _)| DebugTask(&self.0.ctrl.registry, Some(k)))
        //             .filter(|t| {
        //                 match t.1.and_then(|id| t.0.get(id)) {
        //                     Some(task) => task.is_suspended(),
        //                     None => false,
        //                 }
        //             });
        //         if let Some(first) = buff.next() { f.debug_list().entry(&first).entries(buff).finish() } else { write!(f, "None") }
        //     }
        // }
        // writeln!(f, "{:>s$}: {:?}", "suspended", SuspendedDebug(self), s = span)?;
        write!(f, "}}")
    }

    pub(crate) fn poll_internal(&self, cx: &mut Context<'_>) -> Poll<bool> {
        let waker = cx.waker();
        self.last_waker.clear();//drop previous waker if any
        loop {
            if !self.beat_once() {
                //no runnable task found, register waker
                self.last_waker.register(waker);
                //check once again if no task was woken during this time
                if !self.beat_once() {
                    //waiting begins
                    let cnt = self.registry.count();
                    return if cnt == 0 || cnt == self.suspended_count.get() {
                        self.last_waker.clear(); //waker not needed, clear before finishing
                        Poll::Ready(cnt == 0) //true if all tasks finished, false if all suspended
                    } else { Poll::Pending } //all tasks waiting
                }else{
                    //if any was woken then try to deregister waker, then make one rotation
                    self.last_waker.clear();
                }
            }
        }
    }

    fn beat_once(&self) -> bool {
        let mut any_poll = false;
        //capacity is never shortened during execution, even if it will be extended, task allocated outside
        //will be executed by next call to beat_once, note that any_poll will be true in case task is added
        //cause user must execute code for this to happen.
        for (run_key,run_task) in (0..self.registry.capacity()).filter_map(|k|self.registry.get(k).map(move|t|(k,t))) {
            let reason = run_task.get_stop_reason();
            if !reason.is_poll_allowed() {
                if reason == StopReason::Cancelled {
                    drop(run_task);//clear last borrow
                    self.registry.remove(run_key).expect("Internal Error: task not found.");
                }
                continue; //remove from queue
            }
            if !run_task.is_runnable() {
                continue; // next task
            }
            self.current.set(Some(run_key));
            let guard = DropGuard::new(||self.current.set(None));
            // be careful with interior mutability types here cause 'poll_local' can invoke any method
            // on handle, registry is now in borrowed state so nothing can be 'remove'd from it.
            any_poll = true;
            let is_ready = run_task.poll_local().is_ready(); //run user code
            drop(guard);
            if is_ready { //task was finished or cancelled, remove from scheduler
                drop(run_task); //must be dropped!
                self.registry.remove(run_key).expect("Internal Error: task not found.");
            }
        }
        any_poll
    }
}