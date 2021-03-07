use std::ops::Index;
use crate::st::stt_future::StaticFuture;
use std::cell::{Cell, UnsafeCell};
use crate::utils::{AtomicWakerRegistry, Ucw, DropGuard};
use crate::dy::algorithm::TaskKey;
use std::task::{Context, Poll};
use crate::st::handle::StaticHandle;
use std::marker::PhantomData;
use crate::st::StopReason;
use std::mem::forget;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

pub(crate) struct StaticAlgorithm{
    registry: &'static [StaticFuture],
    last_waker: AtomicWakerRegistry,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
    unfinished_count: Cell<usize>,
    current_generation: AtomicUsize,
}

impl StaticAlgorithm{

    pub(crate) const fn from_raw_config(config: &'static [StaticFuture])->Self{
        Self{
            registry: config,
            last_waker: AtomicWakerRegistry::empty(),
            current: Cell::new(None),
            suspended_count: Cell::new(0),
            unfinished_count: Cell::new(usize::MAX),
            current_generation: AtomicUsize::new(usize::MAX), //will become 0 after first init
        }
    }
    pub(crate) fn init(&'static self){ //create all self-refs
        let mut suspended = 0;
        if self.unfinished_count.get() == usize::MAX { //was never initialized
            //all tasks are in uninit state
            for task in self.registry.iter() {
                if task.init(&self.last_waker) {
                    suspended += 1;
                }
            }
        }else{ //reusing previously initialized object
            //all task are in ether uninit or dropped state
            for task in self.registry.iter() {
                task.cancel(StaticHandle::with_id(self,usize::MAX),true); //dummy id
                if task.init(&self.last_waker) {
                    suspended += 1;
                }
            }
        }
        self.suspended_count.set(suspended);
        self.unfinished_count.set(self.registry.len());
        //only need to be volatile increment, init can't be concurrent
        self.current_generation.store(self.current_generation.load(Relaxed).wrapping_add(1),Relaxed);
    }
    pub(crate) fn get_generation(&self)->usize {
        self.current_generation.load(Relaxed) //only volatile read cause it might be read from concurrent threads
    }
    pub(crate) fn get_current(&self) -> Option<TaskKey> { self.current.get() }
    pub(crate) const fn get_registered_count(&self)->usize{self.registry.len()}
    fn inc_suspended(&self) { self.suspended_count.set(self.suspended_count.get() + 1) }
    fn dec_suspended(&self) { self.suspended_count.set(self.suspended_count.get() - 1) }
    fn inc_unfinished(&self) { self.unfinished_count.set(self.unfinished_count.get() + 1) }
    fn dec_unfinished(&self) { self.unfinished_count.set(self.unfinished_count.get() - 1) }
    pub(crate) fn get_name(&self, key: TaskKey) -> Option<&'static str>{
        self.registry.get(key).and_then(|t|t.get_name())
    }
    pub(crate) fn get_by_name(&self, name: &str) -> Option<TaskKey>{
        self.registry.iter().position(|t|t.get_name() == Some(name))
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
            Some(task) => match task.get_stop_reason() {
                StopReason::None => {
                    //todo handle RestartSuspended
                    task.set_stop_reason(StopReason::Suspended);
                    self.inc_suspended();
                    true
                }
                StopReason::Restart => {
                    task.set_stop_reason(StopReason::RestartSuspended);
                    self.inc_suspended();
                    true
                }
                _ => false
            }
            _ => false,
        }
    }
    pub(crate) fn restart(&self, key: TaskKey) -> bool {
        match self.registry.get(key) {
            Some(task) => {
                match task.get_stop_reason() {
                    //if restart suspended, then chang to just restart
                    StopReason::Suspended | StopReason::RestartSuspended => self.dec_suspended(),
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

    pub(crate) fn reset_all_tasks(&'static self){
        if self.unfinished_count.get() == 0 { //no task is alive, lazy reset
            return;//init will handle state changes to tasks, we return here to avoid unnecessary cleanup
        }
        let mut it = self.registry.iter();
        while let Some(task) = it.next() {
            let cleanup = DropGuard::new(||{
                //panic occurred, do emergency cleanup
                while let Some(task) = it.next() {
                    task.cleanup(StaticHandle::with_id(self,usize::MAX)); //aborts on another panic
                }
            });
            task.cleanup(StaticHandle::with_id(self,usize::MAX)); //dummy id
            forget(cleanup);//didn't panic so continue
        }
    }

    pub(crate) fn poll_internal(&'static self, cx: &mut Context<'_>) -> Poll<bool> {
        let waker = cx.waker();
        self.last_waker.clear();//drop previous waker if any
        loop {
            if !self.beat_once() {
                //no runnable task found, register waker
                self.last_waker.register(waker);
                //check once again if no task was woken during this time
                if !self.beat_once() {
                    //waiting begins
                    let cnt = self.unfinished_count.get();
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

    fn beat_once(&'static self) -> bool { //return true if should continue and false if should wait
        let mut any_poll = false;
        let gen_id = self.get_generation();
        for (run_key,run_task) in self.registry.iter().enumerate() {
            let restart = match run_task.get_stop_reason() {
                StopReason::None => false, //don't skip
                StopReason::Cancelled => {
                    run_task.cancel(StaticHandle::with_id(self,usize::MAX),false);
                    self.dec_unfinished();
                    continue; //task cancelled nothing to do
                }
                StopReason::Restart => {
                    run_task.set_stop_reason(StopReason::None);
                    true
                }
                StopReason::RestartSuspended => {
                    run_task.set_stop_reason(StopReason::Suspended);
                    run_task.cancel(StaticHandle::with_id(self,usize::MAX),true); //drop task and set it to uninit
                    continue; //task is suspended so we're done here
                }
                StopReason::Finished | StopReason::Suspended => continue, //not pollable
            };
            if !run_task.is_runnable() {
                continue; // next task
            }
            self.current.set(Some(run_key));
            let guard = DropGuard::new(||self.current.set(None));
            // be careful with interior mutability types here cause 'poll_local' can invoke any method
            // on handle
            any_poll = true;
            let is_ready = run_task.poll_local(StaticHandle::with_id(self,gen_id),restart).is_ready(); //run user code
            drop(guard);
            if is_ready { //task was finished and dropped, mark it
                run_task.set_stop_reason(StopReason::Finished);
                self.dec_unfinished();//one less
            }
        }
        any_poll
    }
}