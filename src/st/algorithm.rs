use std::ops::Index;
use crate::st::stt_future::StaticFuture;
use std::cell::{Cell, UnsafeCell};
use crate::utils::{AtomicWakerRegistry, Ucw, DropGuard};
use crate::dy::algorithm::TaskKey;
use std::task::{Context, Poll};
use crate::st::handle::StaticHandle;
use std::marker::PhantomData;
use crate::st::StopReason;
use crate::st::polling;


pub(crate) struct StaticAlgorithm{
    registry: &'static [StaticFuture],
    last_waker: AtomicWakerRegistry,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
    unfinished_count: Cell<usize>,
}

impl StaticAlgorithm{

    pub(crate) const fn from_raw_config(config: &'static [StaticFuture])->Self{
        Self{
            registry: config,
            last_waker: AtomicWakerRegistry::empty(),
            current: Cell::new(None),
            suspended_count: Cell::new(0),
            unfinished_count: Cell::new(config.len()),
        }
    }
    pub(crate) fn init(&'static self){ //create all self-refs
        let mut suspended = 0;
        for task in self.registry.iter() {
            if task.get_stop_reason() == StopReason::Suspended {
                suspended += 1;
            }
            task.init(&self.last_waker);
        }
        self.suspended_count.set(suspended);
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.current.get() }
    pub(crate) const fn get_registered_count(&self)->usize{self.registry.len()}
    fn inc_suspended(&self) { self.suspended_count.set(self.suspended_count.get() + 1) }
    fn dec_suspended(&self) { self.suspended_count.set(self.suspended_count.get() - 1) }
    fn inc_unfinished(&self) { self.unfinished_count.set(self.unfinished_count.get() + 1) }
    fn dec_unfinished(&self) { self.unfinished_count.set(self.unfinished_count.get() - 1) }

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
                //todo handle RestartSuspended
                task.set_stop_reason(StopReason::Suspended);
                self.inc_suspended();
                true
            }
            _ => false,
        }
    }
    pub(crate) fn restart(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) => {
                match task.get_stop_reason() {
                    StopReason::Suspended => self.dec_suspended(),
                    StopReason::Cancelled | StopReason::Finished => self.inc_unfinished(),
                    StopReason::Restart => return true,//todo decide if user should know about restarting state cause its only market state
                    _ => {}
                }
                task.set_stop_reason(StopReason::Restart);
                true
            }
            _ => false,
        }
    }

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

    pub(crate) fn poll_internal(&'static self, cx: &mut Context<'_>) -> Poll<bool> {
        let waker = cx.waker();
        loop {
            self.last_waker.clear();//drop previous waker if any
            if !self.beat_once() {
                //no runnable task found, register waker
                self.last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if !self.beat_once() {
                    //waiting begins
                    let cnt = self.unfinished_count.get();
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

    fn beat_once(&'static self) -> bool { //return true if should continue and false if should wait
        let registry = &self.registry;
        let handle = StaticHandle::new(self);
        let mut any_poll = false;
        for run_key in 0..registry.len() {
            let run_task = match registry.get(run_key){
                Some(k) => k, None => continue,
            };
            let reason = run_task.get_stop_reason();
            let mut restart = false;
            if reason != StopReason::None {
                if reason == StopReason::Cancelled {
                    run_task.cancel(handle);
                    self.dec_unfinished();
                    continue;
                } else if reason == StopReason::Restart {
                    run_task.set_stop_reason(StopReason::None);
                    restart = true;
                } else {
                    continue; //remove from queue
                }
            }
            if !run_task.is_runnable() {
                continue; // next task
            }
            self.current.set(Some(run_key));
            let guard = DropGuard::new(||self.current.set(None));
            // be careful with interior mutability types here cause 'poll_local' can invoke any method
            // on handle, 'from' queue shouldn't be edited by handles (this is not enforced) and
            // registry is now in borrowed state so nothing can be 'remove'd from it.
            any_poll = true;
            let is_ready = run_task.poll_local(handle,restart).is_ready(); //run user code
            drop(guard);
            if is_ready { //task was finished and dropped, mark it
                run_task.set_stop_reason(StopReason::Finished);
                self.dec_unfinished();//one less
            }
        }
        any_poll
    }
}