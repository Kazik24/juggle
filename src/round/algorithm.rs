use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::fmt::Result;
use core::mem::swap;
use core::task::{Context, Poll, Waker};
use crate::chunk_slab::ChunkSlab;
use crate::round::dyn_future::DynamicFuture;
use crate::round::handle::State;
use crate::utils::AtomicWakerRegistry;
use crate::round::Ucw;

type TaskKey = usize;

pub(crate) struct SchedulerAlgorithm<'futures> {
    runnable: (Ucw<VecDeque<TaskKey>>, Ucw<VecDeque<TaskKey>>),
    last_waker: Arc<AtomicWakerRegistry>,
    which_buffer: Cell<bool>,
    registry: Ucw<ChunkSlab<TaskKey, DynamicFuture<'futures>>>,
    ctrl: Control,
}

struct Control{
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
    deferred: Ucw<Vec<TaskKey>>,
}

impl Control{
    fn inc_suspended(&self){self.suspended_count.set(self.suspended_count.get() + 1)}
    fn dec_suspended(&self){self.suspended_count.set(self.suspended_count.get() - 1)}
}


impl<'futures> SchedulerAlgorithm<'futures> {
    pub(crate) fn new() -> Self {
        Self {
            runnable: (Ucw::new(VecDeque::new()), Ucw::new(VecDeque::new())),
            which_buffer: Cell::new(false),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            registry: Ucw::new(ChunkSlab::new()),
            ctrl: Control{
                deferred: Ucw::new(Vec::new()),
                suspended_count: Cell::new(0),
                current: Cell::new(None),
            },
        }
    }
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            runnable: (Ucw::new(VecDeque::with_capacity(cap)), Ucw::new(VecDeque::with_capacity(cap))),
            which_buffer: Cell::new(false),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            registry: Ucw::new(ChunkSlab::with_capacity(cap)),
            ctrl: Control{
                deferred: Ucw::new(Vec::with_capacity(cap)),
                suspended_count: Cell::new(0),
                current: Cell::new(None),
            },
        }
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.ctrl.current.get() }
    //safe to call from inside task
    fn enqueue_runnable(&self, key: TaskKey, check_absent: bool) {
        if self.which_buffer.get() { //if now 1 is executed then add to 0 and vice versa.
            if check_absent && self.runnable.0.borrow().contains(&key) { return; }
            self.runnable.0.borrow_mut().push_back(key);
        } else {
            if check_absent && self.runnable.1.borrow().contains(&key) { return; }
            self.runnable.1.borrow_mut().push_back(key);
        }
    }
    pub(crate) fn clone_registry(&self) -> Arc<AtomicWakerRegistry> { self.last_waker.clone() }
    //safe to call from inside task because chunkslab never moves futures.
    pub(crate) fn register(&self, dynamic: DynamicFuture<'futures>) -> TaskKey {
        let suspended = dynamic.is_suspended();
        let key = self.registry.borrow_mut().insert(dynamic); //won't realloc other futures because it uses ChunkSlab
        if suspended {
            //increase count cause added task was suspended
            self.ctrl.inc_suspended();
        } else {
            self.enqueue_runnable(key, false); //first ever enqueue of this task
        }
        key
    }
    //safe to call from inside task
    pub(crate) fn resume(&self, key: TaskKey) -> bool {
        let reg = self.registry.borrow();
        match reg.get(key) {
            Some(task) if task.is_suspended() => {
                task.set_suspended(false);
                self.ctrl.dec_suspended();

                if task.is_runnable() {
                    // Check if absent is needed cause some task might spam suspend-resume
                    // which will otherwise cause multiple enqueues.
                    drop(reg);
                    self.enqueue_runnable(key, true);
                } else {//task is still waiting for sth, defer it then
                    // Check if absent as above.
                    let mut deferred = self.ctrl.deferred.borrow_mut();
                    if !deferred.contains(&key) {
                        deferred.push(key);
                    }
                }
                true
            }
            _ => false,
        }
    }

    //if rotate_once encounters suspended task, then it will be removed from queue
    pub(crate) fn suspend(&self, key: TaskKey) -> bool {
        let reg = self.registry.borrow();
        match reg.get(key) {
            Some(task) if !task.is_suspended() => {
                task.set_suspended(true);
                self.ctrl.inc_suspended();
                //optimistic check, if is runnable then for sure will be removed from deferred
                //in next iteration
                if !task.is_runnable() {
                    let mut deferred = self.ctrl.deferred.borrow_mut();
                    if let Some(pos) = deferred.iter().position(|x| *x == key) {
                        deferred.remove(pos);
                    }
                }

                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_state(&self, key: TaskKey) -> State {
        let reg = self.registry.borrow();
        match reg.get(key) {
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
        let reg = self.registry.borrow();
        match reg.get(key) {
            Some(task) if !task.is_cancelled() => {
                task.set_cancelled(true);
                if task.is_suspended() {
                    task.set_suspended(false);
                    self.ctrl.dec_suspended();
                }
                true
            }
            _ => false,
        }
    }
    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey> {
        let reg = self.registry.borrow();
        for (k, v) in reg.iter() {
            match v.get_name_str() {
                Some(n) if n == name => return Some(k),
                _ => {}
            }
        }
        None
    }

    //pub(crate) fn get_dynamic(&self, key: TaskKey) -> Option<UcwRef<DynamicFuture<'futures>>> { self.registry.borrow_mut().get(key).map();}
    pub fn with_name<F, T>(&self, id: TaskKey, func: F) -> T where F: FnOnce(Option<&str>) -> T {
        let reg = self.registry.borrow();
        let arg = reg.get(id).map(|v|v.get_name_str()).flatten();
        func(arg) //todo maybe it panics when closure spawns task (borrowing)
    }

    pub(crate) fn format_internal(&self, f: &mut Formatter<'_>, name: &str) -> Result {
        pub(crate) struct DebugTask<'a, 'b>(
            &'a ChunkSlab<TaskKey, DynamicFuture<'b>>,
            Option<TaskKey>,
        );

        impl<'a, 'b> Debug for DebugTask<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self.1 {
                    Some(id) => {
                        if let Some(task) = self.0.get(id) {
                            return match task.get_name_str() {
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
        writeln!(f, "{:>s$}: {:?}", "current", DebugTask(&self.registry.borrow(), self.ctrl.current.get()), s = span)?;

        struct RunnableDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for RunnableDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                if self.0.runnable.0.borrow().is_empty() && self.0.runnable.1.borrow().is_empty() { write!(f, "None") } else {
                    let mut buff0 = &self.0.runnable.0.borrow();
                    let mut buff1 = &self.0.runnable.1.borrow();
                    if self.0.which_buffer.get() { swap(&mut buff0, &mut buff1); }
                    let reg = self.0.registry.borrow();
                    let buff0 = buff0.iter().map(|&k| DebugTask(&reg, Some(k)));
                    let buff1 = buff1.iter().map(|&k| DebugTask(&reg, Some(k)));
                    f.debug_list().entries(buff0).entries(buff1).finish()
                }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "runnable", RunnableDebug(self), s = span)?;

        struct WaitingDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for WaitingDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let deferred = self.0.ctrl.deferred.borrow();
                let registry = &self.0.registry.borrow();
                let buff = deferred.iter().map(|&k| DebugTask(registry, Some(k)));
                if deferred.is_empty() { write!(f, "None") } else {
                    f.debug_list().entries(buff).finish()
                }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "waiting", WaitingDebug(self), s = span)?;

        struct SuspendedDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for SuspendedDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let reg = &self.0.registry.borrow();
                let mut buff = reg.iter().map(|(k, _)| DebugTask(reg, Some(k)))
                    .filter(|t| {
                        match t.1.map(|id| t.0.get(id)).flatten() {
                            Some(task) => task.is_suspended(),
                            None => false,
                        }
                    });
                if let Some(first) = buff.next() { f.debug_list().entry(&first).entries(buff).finish() } else { write!(f, "None") }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "suspended", SuspendedDebug(self), s = span)?;
        write!(f, "}}")
    }

    pub(crate) fn poll_internal(&self, cx: &mut Context<'_>) -> Poll<bool> {
        loop {
            self.last_waker.clear();//drop previous waker if any
            if self.which_buffer.get() {
                if Self::beat_once(&self.registry, &self.ctrl, &self.runnable.1, &self.runnable.0,
                                   cx.waker(), &self.last_waker) {
                    return Poll::Pending;
                }
            } else {
                if Self::beat_once(&self.registry, &self.ctrl, &self.runnable.0, &self.runnable.1,
                                   cx.waker(), &self.last_waker) {
                    return Poll::Pending;
                }
            }
            self.which_buffer.set(!self.which_buffer.get());
            if self.runnable.0.borrow().is_empty() && self.runnable.1.borrow().is_empty()
                && self.ctrl.deferred.borrow().is_empty() { break; }
        }
        if self.ctrl.suspended_count.get() != 0 { Poll::Ready(false) } //all tasks are suspended
        else { Poll::Ready(true) } //all tasks executed to finish
    }

    fn beat_once(registry: &Ucw<ChunkSlab<TaskKey, DynamicFuture>>,ctrl: &Control,
                 from: &Ucw<VecDeque<TaskKey>>, to: &Ucw<VecDeque<TaskKey>>,
                 waker: &Waker, last_waker: &AtomicWakerRegistry) -> bool {
        let deferred = &mut ctrl.deferred.borrow_mut();
        let ref_from = &mut from.borrow_mut();
        let reg = &registry.borrow();
        if !deferred.is_empty() && !Self::drain_runnable(reg, deferred, ref_from) {
            if ref_from.is_empty() { //if has no work to do
                //no runnable task found, register waker
                last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if !Self::drain_runnable(reg, deferred, ref_from) {
                    //waiting begins
                    return true;//true means that future should wait for waker
                }
                //if any was woken then try to deregister waker, then make one rotation
                last_waker.clear();
            }
        }

        Self::rotate_once(registry, ctrl, from, to);
        false //can start new iteration
    }

    #[inline]
    fn rotate_once(registry: &Ucw<ChunkSlab<TaskKey, DynamicFuture>>,ctrl: &Control,
                   from: &Ucw<VecDeque<TaskKey>>, to: &Ucw<VecDeque<TaskKey>>) {
        struct Guard<'a>(&'a Cell<Option<TaskKey>>);
        impl<'a> Drop for Guard<'a> { fn drop(&mut self) { self.0.set(None); } }

        while let Some(run_key) = from.borrow_mut().pop_front() {
            let mut reg_ref = registry.borrow_mut();
            let run_task = reg_ref.get(run_key).unwrap();
            if run_task.is_cancelled() {
                //dropping future might be a problem
                reg_ref.remove(run_key).expect("Internal Error: task not found.");
                continue; //remove from queue and registry
            }
            if run_task.is_suspended() {
                continue; // remove from queue
            }

            ctrl.current.set(Some(run_key));
            let guard = Guard(&ctrl.current);
            // be careful with interior mutability types here cause poll_local can invoke any method
            // on handle, therefore 'from' queue shouldn't be edited by handles (other structures
            // are pretty much ok)
            let result = run_task.poll_local().is_pending(); //run user code
            drop(guard);

            if result { //reconsider enqueuing this future again
                if run_task.is_runnable() { //if immediately became runnable then enqueue it
                    to.borrow_mut().push_back(run_key);
                } else { //put it on deferred queue
                    ctrl.deferred.borrow_mut().push(run_key);
                }
            } else { //task was finished, remove from scheduler
                drop(reg_ref); //must be dropped!
                registry.borrow_mut().remove(run_key).expect("Internal Error: task not found.");
            }
        }
    }

    fn drain_runnable(registry: &ChunkSlab<TaskKey, DynamicFuture<'_>>,
                      from: &mut Vec<TaskKey>, to: &mut VecDeque<TaskKey>) -> bool {
        let prev = from.len();
        from.retain(|&elem| {
            let task = registry.get(elem).unwrap();
            if task.is_runnable() {
                to.push_back(elem);
                return false;
            }
            true
        });
        prev != from.len()
    }
}