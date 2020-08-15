use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::{Debug, Formatter};
use core::fmt::Result;
use core::mem::swap;
use core::task::{Context, Poll, Waker};
use crate::chunk_slab::ChunkSlab;
use crate::round::dyn_future::DynamicFuture;
use crate::round::handle::State;
use crate::utils::AtomicWakerRegistry;

type TaskKey = usize;

pub(crate) struct SchedulerAlgorithm<'futures> {
    runnable0: VecDeque<TaskKey>,
    runnable1: VecDeque<TaskKey>,
    deferred: Vec<TaskKey>,
    last_waker: Arc<AtomicWakerRegistry>,
    pub(crate) registry: ChunkSlab<TaskKey, DynamicFuture<'futures>>,
    suspended_count: usize,
    current: Option<TaskKey>,
    which_buffer: bool,
}

impl<'futures> SchedulerAlgorithm<'futures> {
    pub(crate) fn new() -> Self {
        Self {
            runnable0: VecDeque::new(),
            runnable1: VecDeque::new(),
            which_buffer: false,
            deferred: Vec::new(),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            suspended_count: 0,
            current: None,
            registry: ChunkSlab::new(),
        }
    }
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            runnable0: VecDeque::with_capacity(cap),
            runnable1: VecDeque::with_capacity(cap),
            which_buffer: false,
            deferred: Vec::with_capacity(cap),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            suspended_count: 0,
            current: None,
            registry: ChunkSlab::with_capacity(cap),
        }
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.current }
    //safe to call from inside task
    fn enqueue_runnable(&mut self, key: TaskKey, check_absent: bool) {
        if self.which_buffer { //if now 1 is executed then add to 0 and vice versa.
            if check_absent && self.runnable0.contains(&key) { return; }
            self.runnable0.push_back(key);
        } else {
            if check_absent && self.runnable1.contains(&key) { return; }
            self.runnable1.push_back(key);
        }
    }
    pub(crate) fn clone_registry(&self) -> Arc<AtomicWakerRegistry> { self.last_waker.clone() }
    //safe to call from inside task because chunkslab never moves futures.
    pub(crate) fn register(&mut self, dynamic: DynamicFuture<'futures>) -> TaskKey {
        let suspended = dynamic.is_suspended();
        let key = self.registry.insert(dynamic); //won't realloc other futures because it uses ChunkSlab
        if suspended {
            self.suspended_count += 1; //increase count cause added task was suspended
        } else {
            self.enqueue_runnable(key, false); //first ever enqueue of this task
        }
        key
    }
    //safe to call from inside task
    pub(crate) fn resume(&mut self, key: TaskKey) -> bool {
        match self.registry.get_mut(key) {
            Some(task) if task.is_suspended() => {
                task.set_suspended(false);
                self.suspended_count -= 1;

                if task.is_runnable() {
                    // Check if absent is needed cause some task might spam suspend-resume
                    // which will otherwise cause multiple enqueues.
                    self.enqueue_runnable(key, true);
                } else {//task is still waiting for sth, defer it then
                    // Check if absent as above.
                    if !self.deferred.contains(&key) {
                        self.deferred.push(key);
                    }
                }
                true
            }
            _ => false,
        }
    }

    //if rotate_once encounters suspended task, then it will be removed from queue
    pub(crate) fn suspend(&mut self, key: TaskKey) -> bool {
        match self.registry.get_mut(key) {
            Some(task) if !task.is_suspended() => {
                task.set_suspended(true);
                self.suspended_count += 1;
                //optimistic check, if is runnable then for sure will be removed from deferred
                //in next iteration
                if !task.is_runnable() {
                    if let Some(pos) = self.deferred.iter().position(|x| *x == key) {
                        self.deferred.remove(pos);
                    }
                }

                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_state(&self, key: TaskKey) -> State {
        match self.registry.get(key) {
            Some(task) => {
                if task.is_cancelled() { State::Cancelled } else if task.is_suspended() { State::Suspended } else if task.is_runnable() { State::Runnable } else { State::Waiting }
            }
            None => State::Unknown,
        }
    }

    //if rotate_once encounters cancelled task, then it will be removed from queue and registry
    pub(crate) fn cancel(&mut self, key: TaskKey) -> bool {
        match self.registry.get_mut(key) {
            Some(task) if !task.is_cancelled() => {
                task.set_cancelled(true);
                if task.is_suspended() {
                    task.set_suspended(false);
                    self.suspended_count -= 1;
                }
                true
            }
            _ => false,
        }
    }
    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey> {
        for (k, v) in self.registry.iter() {
            match v.get_name_str() {
                Some(n) if n == name => return Some(k),
                _ => {}
            }
        }
        None
    }

    pub(crate) fn get_dynamic(&self, key: TaskKey) -> Option<&DynamicFuture<'futures>> { self.registry.get(key) }

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
        writeln!(f, "{:>s$}: {:?}", "current", DebugTask(&self.registry, self.current), s = span)?;

        struct RunnableDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for RunnableDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                if self.0.runnable0.is_empty() && self.0.runnable1.is_empty() { write!(f, "None") } else {
                    let mut buff0 = &self.0.runnable0;
                    let mut buff1 = &self.0.runnable1;
                    if self.0.which_buffer { swap(&mut buff0, &mut buff1); }
                    let buff0 = buff0.iter().map(|&k| DebugTask(&self.0.registry, Some(k)));
                    let buff1 = buff1.iter().map(|&k| DebugTask(&self.0.registry, Some(k)));
                    f.debug_list().entries(buff0).entries(buff1).finish()
                }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "runnable", RunnableDebug(self), s = span)?;

        struct WaitingDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for WaitingDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let buff = self.0.deferred.iter().map(|&k| DebugTask(&self.0.registry, Some(k)));
                if self.0.deferred.is_empty() { write!(f, "None") } else { f.debug_list().entries(buff).finish() }
            }
        }
        writeln!(f, "{:>s$}: {:?}", "waiting", WaitingDebug(self), s = span)?;

        struct SuspendedDebug<'a, 'b>(&'a SchedulerAlgorithm<'b>);
        impl<'a, 'b> Debug for SuspendedDebug<'a, 'b> {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let mut buff = self.0.registry.iter().map(|(k, _)| DebugTask(&self.0.registry, Some(k)))
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

    pub(crate) fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<bool> {
        loop {
            self.last_waker.clear();//drop previous waker if any
            if self.which_buffer {
                if Self::beat_once(&mut self.registry, &mut self.runnable1, &mut self.runnable0,
                                   &mut self.deferred, cx.waker(), &self.last_waker, &mut self.current) {
                    return Poll::Pending;
                }
            } else {
                if Self::beat_once(&mut self.registry, &mut self.runnable0, &mut self.runnable1,
                                   &mut self.deferred, cx.waker(), &self.last_waker, &mut self.current) {
                    return Poll::Pending;
                }
            }
            self.which_buffer = !self.which_buffer;
            if self.runnable0.is_empty() && self.runnable1.is_empty() && self.deferred.is_empty() { break; }
        }
        if self.suspended_count != 0 { Poll::Ready(false) } //all tasks are suspended
        else { Poll::Ready(true) } //all tasks executed to finish
    }

    fn beat_once(registry: &mut ChunkSlab<TaskKey, DynamicFuture<'_>>,
                 from: &mut VecDeque<TaskKey>, to: &mut VecDeque<TaskKey>,
                 deferred: &mut Vec<TaskKey>, waker: &Waker,
                 last_waker: &AtomicWakerRegistry, current: &mut Option<TaskKey>) -> bool {
        if !deferred.is_empty() && !Self::drain_runnable(registry, deferred, from) {
            if from.is_empty() { //if has no work to do
                //no runnable task found, register waker
                last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if !Self::drain_runnable(registry, deferred, from) {
                    //waiting begins
                    return true;//true means that future should wait for waker
                }
                //if any was woken then try to deregister waker, then make one rotation
                last_waker.clear();
            }
        }
        Self::rotate_once(registry, from, to, deferred, current);
        false //can start new iteration
    }

    fn rotate_once(registry: &mut ChunkSlab<TaskKey, DynamicFuture<'_>>, from: &mut VecDeque<TaskKey>,
                   to: &mut VecDeque<TaskKey>, deferred: &mut Vec<TaskKey>,
                   current: &mut Option<TaskKey>) {
        struct Guard<'a>(&'a mut Option<TaskKey>);
        impl<'a> Drop for Guard<'a> { fn drop(&mut self) { *self.0 = None; } }

        while let Some(run_key) = from.pop_front() {
            let run_task = registry.get_mut(run_key).unwrap();
            if run_task.is_cancelled() {
                registry.remove(run_key).expect("Internal Error: task not found.");
                continue; //remove from queue and registry
            }
            if run_task.is_suspended() {
                continue; // remove from queue
            }
            *current = Some(run_key);
            let guard = Guard(current);
            // be careful with interior mutability types here cause poll_local can invoke any method
            // on handle, therefore 'from' queue shouldn't be edited by handles (other structures
            // are pretty much ok)
            let result = run_task.poll_local().is_pending(); //run user code
            drop(guard);

            if result { //reconsider enqueuing this future again
                if run_task.is_runnable() { //if immediately became runnable then enqueue it
                    to.push_back(run_key);
                } else { //put it on deferred queue
                    deferred.push(run_key);
                }
            } else { //task was finished, remove from scheduler
                registry.remove(run_key).expect("Internal Error: task not found.");
            }
        }
    }

    fn drain_runnable(registry: &mut ChunkSlab<TaskKey, DynamicFuture<'_>>,
                      from: &mut Vec<TaskKey>, to: &mut VecDeque<TaskKey>) -> bool {
        let prev = from.len();
        from.retain(|&elem| {
            let task = registry.get_mut(elem).unwrap();
            if task.is_runnable() {
                to.push_back(elem);
                return false;
            }
            true
        });
        prev != from.len()
    }
}