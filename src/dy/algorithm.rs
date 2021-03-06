use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::fmt::Result;
use core::mem::swap;
use core::task::{Context, Poll, Waker};
use crate::dy::dyn_future::{DynamicFuture, TaskName};
use crate::dy::handle::State;
use crate::dy::registry::Registry;
use crate::utils::Ucw;
use crate::utils::AtomicWakerRegistry;
use crate::dy::stat::{TaskWrapper, StopReason, TaskRegistry};

pub(crate) type TaskKey = usize;

pub(crate) struct SchedulerAlgorithm<'futures> {
    runnable: (Ucw<VecDeque<TaskKey>>, Ucw<VecDeque<TaskKey>>),
    which_buffer: Cell<bool>,
    ctrl: Control<'futures>,
}

struct Control<'futures> {
    registry: Registry<'futures>,
    last_waker: Arc<AtomicWakerRegistry>,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
    deferred: Ucw<Vec<TaskKey>>,
    scan_registry: Cell<bool>,
}

#[repr(u8)]
enum Rotate { Wait, Continue }


impl<'futures> SchedulerAlgorithm<'futures> {
    pub(crate) fn new() -> Self {
        Self {
            runnable: (Ucw::new(VecDeque::new()), Ucw::new(VecDeque::new())),
            which_buffer: Cell::new(false),
            ctrl: Control {
                registry: Registry::new(),
                last_waker: Arc::new(AtomicWakerRegistry::empty()),
                deferred: Ucw::new(Vec::new()),
                suspended_count: Cell::new(0),
                current: Cell::new(None),
                scan_registry: Cell::new(false),
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
    pub(crate) fn clone_registry(&self) -> Arc<AtomicWakerRegistry> { self.ctrl.last_waker.clone() }
    //safe to call from inside task because chunkslab never moves futures.
    pub(crate) fn register(&self, dynamic: DynamicFuture<'futures>) -> Option<TaskKey> {
        let suspended = dynamic.get_stop_reason() == StopReason::Suspended;
        let key = self.ctrl.registry.insert(dynamic); //won't realloc other futures because it uses ChunkSlab
        if let Some(key) = key {
            if suspended {
                //increase count cause added task was suspended
                self.ctrl.inc_suspended();
            } else {
                self.enqueue_runnable(key, false); //first ever enqueue of this task
            }
        }
        key
    }
    //safe to call from inside task
    pub(crate) fn resume(&self, key: TaskKey) -> bool {
        match self.ctrl.registry.get(key) {
            Some(task) if task.get_stop_reason() == StopReason::Suspended => {
                task.set_stop_reason(StopReason::None);
                self.ctrl.dec_suspended();

                if task.is_runnable() || self.ctrl.current.get() == Some(key) {
                    // Check if absent is needed cause some task might spam suspend-resume
                    // which will otherwise cause multiple enqueues.
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
        match self.ctrl.registry.get(key) {
            Some(task) if task.get_stop_reason() == StopReason::None => {
                task.set_stop_reason(StopReason::Suspended);
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
        match self.ctrl.registry.get(key) {
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
        match self.ctrl.registry.get(key) {
            Some(task) => {
                let r = task.get_stop_reason();
                if r != StopReason::Cancelled {
                    task.set_stop_reason(StopReason::Cancelled);
                    if r == StopReason::Suspended {
                        self.ctrl.dec_suspended();
                        self.ctrl.scan_registry.set(true);
                    }
                    true
                } else { false }
            }
            _ => false,
        }
    }
    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey> {
        for (k, v) in self.ctrl.registry.iter() {
            match v.get_name().as_str() {
                Some(n) if n == name => return Some(k),
                _ => {}
            }
        }
        None
    }
    pub(crate) fn registered_count(&self)->usize{ self.ctrl.registry.count() }

    pub fn with_name<F, T>(&self, id: TaskKey, func: F) -> T where F: FnOnce(&TaskName) -> T {
        match self.ctrl.registry.get(id) {
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
        writeln!(f, "{:>s$}: {:?}", "current", DebugTask(&self.ctrl.registry, self.ctrl.current.get()), s = span)?;

        struct RunnableDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for RunnableDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                if self.0.runnable.0.borrow().is_empty() && self.0.runnable.1.borrow().is_empty() { write!(f, "None") } else {
                    let mut buff0 = &self.0.runnable.0.borrow();
                    let mut buff1 = &self.0.runnable.1.borrow();
                    if self.0.which_buffer.get() { swap(&mut buff0, &mut buff1); }
                    let buff0 = buff0.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
                    let buff1 = buff1.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
                    f.debug_list().entries(buff0).entries(buff1).finish()
                }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "runnable", RunnableDebug(self), s = span)?;

        struct WaitingDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for WaitingDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let deferred = self.0.ctrl.deferred.borrow();
                let buff = deferred.iter().map(|&k| DebugTask(&self.0.ctrl.registry, Some(k)));
                if deferred.is_empty() { write!(f, "None") } else {
                    f.debug_list().entries(buff).finish()
                }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "waiting", WaitingDebug(self), s = span)?;

        struct SuspendedDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for SuspendedDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let mut buff = self.0.ctrl.registry.iter().map(|(k, _)| DebugTask(&self.0.ctrl.registry, Some(k)))
                    .filter(|t| {
                        match t.1.and_then(|id| t.0.get(id)) {
                            Some(task) => task.get_stop_reason() == StopReason::Suspended,
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
            self.ctrl.last_waker.clear();//drop previous waker if any
            let beat_result = if self.which_buffer.get() {
                self.ctrl.beat_once(&self.runnable.1, &self.runnable.0, cx.waker())
            } else {
                self.ctrl.beat_once(&self.runnable.0, &self.runnable.1, cx.waker())
            };
            self.which_buffer.set(!self.which_buffer.get());
            if let Rotate::Wait = beat_result {
                return Poll::Pending;
            }
            if self.runnable.0.borrow().is_empty() && self.runnable.1.borrow().is_empty()
                && self.ctrl.deferred.borrow().is_empty() { break; }
        }
        if self.ctrl.suspended_count.get() != 0 { Poll::Ready(false) } //all tasks are suspended
        else { Poll::Ready(true) } //all tasks executed to finish
    }
}

impl Control<'_> {
    fn inc_suspended(&self) { self.suspended_count.set(self.suspended_count.get() + 1) }
    fn dec_suspended(&self) { self.suspended_count.set(self.suspended_count.get() - 1) }

    #[inline]
    fn process_tasks(&self, from: &Ucw<VecDeque<TaskKey>>, to: &Ucw<VecDeque<TaskKey>>, waker: &Waker) -> Rotate {
        let from = &mut from.borrow_mut();
        let deferred = &mut self.deferred.borrow_mut();
        if !deferred.is_empty() && !Self::drain_runnable(&self.registry, deferred, from) {
            if from.is_empty() && to.borrow().is_empty() { //if has no work to do
                //no runnable task found, register waker
                self.last_waker.register(waker);
                //check once again if no task was woken during this time
                if !Self::drain_runnable(&self.registry, deferred, from) {
                    //waiting begins
                    return Rotate::Wait;//means that future should wait for waker
                }
                //if any was woken then try to deregister waker, then make one rotation
                self.last_waker.clear();
            }
        }
        Rotate::Continue
    }
    fn beat_once(&self, from: &Ucw<VecDeque<TaskKey>>, to: &Ucw<VecDeque<TaskKey>>, waker: &Waker) -> Rotate {
        match self.process_tasks(from, to, waker) {
            Rotate::Wait => Rotate::Wait,//indicates that future should wait for waker now
            Rotate::Continue => {
                self.rotate_once(from, to);
                Rotate::Continue //can start new iteration
            }
        }
    }

    #[inline]
    fn peek_front_queue(queue: &Ucw<VecDeque<TaskKey>>) -> Option<TaskKey>{
        queue.borrow().front().copied() //separate fn to drop borrow
    }
    #[inline]
    fn rotate_once(&self, from: &Ucw<VecDeque<TaskKey>>, to: &Ucw<VecDeque<TaskKey>>) {
        struct Guard<'a>(&'a Cell<Option<TaskKey>>,TaskKey,&'a Ucw<VecDeque<TaskKey>>,&'a Ucw<VecDeque<TaskKey>>);
        impl<'a> Drop for Guard<'a> {
            fn drop(&mut self) {
                self.0.set(None);
                let mut from = self.2.borrow_mut();
                match from.front() { //remove from queue only if it wasn't removed inside task
                    Some(&key) if key == self.1 => {
                        from.pop_front(); //regular pop, key is in the same place
                    }
                    _ => {
                        //need to scan 'to' queue to find if task was re-added
                        drop(from);
                        let mut to = self.3.borrow_mut();
                        if let Some(index) = to.iter().position(|&k|k == self.1){
                            to.remove(index);
                        }
                    }
                }
            }
        }

        while let Some(run_key) = Self::peek_front_queue(from) {
            let run_task = self.registry.get(run_key).expect("Internal Error: unknown task in queue.");
            let reason = run_task.get_stop_reason();
            if reason != StopReason::None {
                if reason == StopReason::Cancelled {
                    drop(run_task);//clear last borrow
                    self.registry.remove(run_key).expect("Internal Error: task not found.");
                }
                continue; //remove from queue
            }

            self.current.set(Some(run_key));
            let guard = Guard(&self.current,run_key,from,to);
            // be careful with interior mutability types here cause 'poll_local' can invoke any method
            // on handle, 'from' queue shouldn't be edited by handles (this is not enforced) and
            // registry is now in borrowed state so nothing can be 'remove'd from it.
            let is_ready = run_task.poll_local().is_ready(); //run user code
            drop(guard);

            if is_ready || run_task.get_stop_reason() == StopReason::Cancelled { //task was finished or cancelled, remove from scheduler
                drop(run_task); //must be dropped!
                self.registry.remove(run_key).expect("Internal Error: task not found.");
            } else if run_task.get_stop_reason() != StopReason::Suspended { //reconsider enqueuing this future again
                if run_task.is_runnable() { //if immediately became runnable then enqueue it
                    to.borrow_mut().push_back(run_key);
                } else { //put it on deferred queue
                    self.deferred.borrow_mut().push(run_key);
                }
            }
        }
        if self.scan_registry.get() { //perform scan
            self.scan_registry.set(false); //clear flag
            //todo make this more efficient
            //from queue is now empty
            //but to queue must be checked if it contains any cancelled tasks
            to.borrow_mut().retain(|&key| self.registry.get(key).unwrap().get_stop_reason() != StopReason::Cancelled);
            //now registry can be cleared
            self.registry.retain(|_,v| v.get_stop_reason() != StopReason::Cancelled);
        }
    }

    fn drain_runnable(registry: &Registry<'_>,
                      from: &mut Vec<TaskKey>, to: &mut VecDeque<TaskKey>) -> bool {
        let prev = from.len();
        from.retain(|&elem| {
            let task = registry.get(elem).unwrap();
            if task.get_stop_reason() == StopReason::Suspended { return false; }
            if task.get_stop_reason() == StopReason::Cancelled {
                drop(task);
                registry.remove(elem).expect("Internal Error: task not found.");
                return false;
            }
            if task.is_runnable() {
                to.push_back(elem);
                return false;
            }
            true
        });
        prev != from.len()
    }
}