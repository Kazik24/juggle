use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::fmt::Result;
use core::mem::swap;
use core::task::{Context, Poll, Waker};
use crate::round::dyn_future::{DynamicFuture, TaskName};
use crate::round::handle::State;
use crate::round::registry::Registry;
use crate::round::Ucw;
use crate::utils::{AtomicWakerRegistry, DropGuard};

pub(crate) type TaskKey = usize;


pub(crate) struct UnorderedAlgorithm<'futures>{
    registry: Registry<'futures>,
    last_waker: Arc<AtomicWakerRegistry>,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
}
#[repr(u8)]
enum Rotate { Wait, Continue }

impl<'futures> UnorderedAlgorithm<'futures>{
    pub(crate) fn new() -> Self {
        Self {
            registry: Registry::new(),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            suspended_count: Cell::new(0),
            current: Cell::new(None),
        }
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.current.get() }
    pub(crate) fn clone_registry(&self) -> Arc<AtomicWakerRegistry> { self.last_waker.clone() }
    fn inc_suspended(&self) { self.suspended_count.set(self.suspended_count.get() + 1) }
    fn dec_suspended(&self) { self.suspended_count.set(self.suspended_count.get() - 1) }
    pub(crate) fn register(&self, dynamic: DynamicFuture<'futures>) -> Option<TaskKey> {
        let suspended = dynamic.is_suspended();
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
            Some(task) if task.is_suspended() && !task.is_cancelled() => {
                task.set_suspended(false);
                self.dec_suspended();
                true
            }
            _ => false,
        }
    }

    //if rotate_once encounters suspended task, then it will be removed from queue
    pub(crate) fn suspend(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) if !task.is_suspended() && !task.is_cancelled() => {
                task.set_suspended(true);
                self.inc_suspended();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_state(&self, key: TaskKey) -> State {
        match self.registry.get(key) {
            Some(task) => {
                if task.is_cancelled() { State::Cancelled }
                else if task.is_suspended() { State::Suspended }
                else if task.is_runnable() { State::Runnable }
                else { State::Waiting }
            }
            None => State::Unknown,
        }
    }

    //if rotate_once encounters cancelled task, then it will be removed from queue and registry
    pub(crate) fn cancel(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) if !task.is_cancelled() => {
                task.cancel();
                if task.is_suspended() {
                    task.set_suspended(false);
                    self.dec_suspended();
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey> {
        for (k, v) in self.registry.iter() {
            match v.get_name().as_str() {
                Some(n) if n == name => return Some(k),
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
        pub(crate) struct DebugTask<'a, 'b>(
            &'a Registry<'b>,
            Option<TaskKey>,
        );

        impl<'a, 'b> Debug for DebugTask<'a, 'b> {
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
        loop {
            self.last_waker.clear();//drop previous waker if any
            let beat_result = self.beat_once();
            if let Rotate::Wait = beat_result {
                //no runnable task found, register waker
                self.last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if let Rotate::Wait = self.beat_once() {
                    //waiting begins
                    let cnt = self.registry.count();
                    return if cnt == 0 { Poll::Ready(true) }
                    else if cnt == self.suspended_count.get() { Poll::Ready(false) }//all tasks are suspended
                    else { Poll::Pending } //all tasks executed to finish
                }else{
                    //if any was woken then try to deregister waker, then make one rotation
                    self.last_waker.clear();
                }
            }
        }
    }

    fn beat_once(&self) -> Rotate {
        let cap = self.registry.capacity();
        let mut any_poll = false;
        for run_key in 0..cap {
            let run_task = match self.registry.get(run_key){
                Some(k) => k, None => continue,
            };
            if run_task.is_cancelled() {
                drop(run_task);//clear last borrow
                self.registry.remove(run_key).expect("Internal Error: task not found.");
                continue;
            }
            if run_task.is_suspended() || !run_task.is_runnable() {
                continue; // next task
            }
            self.current.set(Some(run_key));
            let guard = DropGuard::new(||self.current.set(None));
            // be careful with interior mutability types here cause 'poll_local' can invoke any method
            // on handle, 'from' queue shouldn't be edited by handles (this is not enforced) and
            // registry is now in borrowed state so nothing can be 'remove'd from it.
            any_poll = true;
            let is_ready = run_task.poll_local().is_ready(); //run user code
            drop(guard);
            if is_ready { //task was finished or cancelled, remove from scheduler
                drop(run_task); //must be dropped!
                self.registry.remove(run_key).expect("Internal Error: task not found.");
            }
        }
        if any_poll { Rotate::Continue } else { Rotate::Wait }
    }
}