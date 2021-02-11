use std::cell::Cell;
use crate::round::stat::{StopReason, TaskWrapper};
use crate::round::dyn_future::TaskName;
use std::task::{Context, Poll, Waker};
use crate::utils::{DropGuard, AtomicWakerRegistry, DynamicWake, to_waker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) struct StaticFuture{
    //not send not sync
    static_poll: unsafe fn(&mut Context<'_>) ->Poll<()>,
    flags: StaticSyncFlags,
    name: TaskName,
    stop_reason: Cell<StopReason>,
    polling: Cell<bool>,
}

impl TaskWrapper for StaticFuture{
    fn get_name(&self) -> &TaskName { &self.name }
    fn get_stop_reason(&self) -> StopReason { self.stop_reason.get() }
    fn set_stop_reason(&self, val: StopReason) { self.stop_reason.set(val); }
    fn is_runnable(&self) -> bool { self.flags.is_runnable() }
    fn poll_local(&self) -> Poll<()> {
        //SAFETY: guard against undefined behavior of borrowing UnsafeCell mutably twice.
        if self.polling.replace(true) {
            panic!("Recursive call to StaticFuture::poll_local is not allowed.");
        }
        let guard = DropGuard::new(||self.polling.set(false)); //SAFETY: construct guard

        //store false cause if it became true before this operation then polling can be done
        //if it becomes true after this operation but before poll then this also means that polling can be done
        self.flags.set_runnable(false);

        //SAFETY: we just checked if this function was called recursively.
        let result = unsafe {
            let func = self.static_poll;
            func(&mut Context::from_waker(self.flags.waker_ref()))
        };
        drop(guard); //explicit drop
        result
    }
}


struct StaticSyncFlags {
    global: &'static AtomicWakerRegistry,
    runnable: AtomicBool,
    // inline waker cause we want to avoid cloning (optimally i would like this to be only field cause
    // this waker is actually wrapped arc from above, but you cant access inner content of waker
    // without using transmute, so i don't want to do that)
    waker: Waker,
}

impl StaticSyncFlags {
    fn new(global: &'static AtomicWakerRegistry) -> Self {
        // let flags = Arc::new(InnerSyncFlags {
        //     global,
        //     runnable: AtomicBool::new(true),
        // });
        Self {
            waker: to_waker(Arc::new(||{})),
            global,
            runnable: AtomicBool::new(true),
        }
    }
    fn waker_ref(&self) -> &Waker { &self.waker }
    fn is_runnable(&self) -> bool { self.runnable.load(Ordering::Relaxed) }
    fn set_runnable(&self, value: bool) { self.runnable.store(value, Ordering::Release) }
}

struct InnerSyncFlags {
    global: Arc<AtomicWakerRegistry>,
    runnable: AtomicBool,
}

impl DynamicWake for InnerSyncFlags {
    fn wake(&self) {
        self.runnable.store(true, Ordering::Release);
        self.global.notify_wake();
    }
}
