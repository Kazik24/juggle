use std::cell::Cell;
use crate::dy::stat::{StopReason, TaskWrapper};
use crate::dy::dyn_future::TaskName;
use std::task::{Context, Poll, Waker};
use crate::utils::{DropGuard, AtomicWakerRegistry, DynamicWake, to_waker, to_static_waker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crate::st::wheel::StaticHandle;
use std::mem::MaybeUninit;

pub(crate) struct StaticFuture{
    //not send not sync
    static_poll: unsafe fn(StaticHandle,&mut Context<'_>,bool) ->Poll<()>,
    flags: Option<StaticSyncFlags>,
    name: Option<&'static str>,
    stop_reason: Cell<StopReason>,
    polling: Cell<bool>,
}

impl StaticFuture{
    pub const fn new(poll: unsafe fn(StaticHandle,&mut Context<'_>,bool) ->Poll<()>,
                     name: Option<&'static str>,suspended: bool)->Self{
        Self{
            static_poll: poll,
            flags: None,
            name,
            stop_reason: Cell::new(if suspended {StopReason::Suspended} else {StopReason::None}),
            polling: Cell::new(false),
        }
    }
    pub(crate)fn init(&mut self,global: &'static AtomicWakerRegistry){
        self.flags = Some(StaticSyncFlags::new(global));
    }

    pub(crate)fn get_name(&self) -> Option<&str> { self.name }
    pub(crate)fn get_stop_reason(&self) -> StopReason { self.stop_reason.get() }
    pub(crate)fn set_stop_reason(&self, val: StopReason) { self.stop_reason.set(val); }
    pub(crate)fn is_runnable(&self) -> bool { self.flags.as_ref().expect("StaticFuture init error").is_runnable() }
    pub(crate)fn poll_local(&'static self,handle: StaticHandle,restart: bool) -> Poll<()> {
        //SAFETY: guard against undefined behavior of recursive polling.
        if self.polling.replace(true) {
            panic!("Recursive call to StaticFuture::poll_local is not allowed.");
        }
        let guard = DropGuard::new(||self.polling.set(false)); //SAFETY: construct guard

        //store false cause if it became true before this operation then polling can be done
        //if it becomes true after this operation but before poll then this also means that polling can be done
        let flags = self.flags.as_ref().expect("StaticFuture init error");
        flags.set_runnable(false);

        //SAFETY: we just checked if this function was called recursively.
        let result = unsafe {
            let func = self.static_poll;
            let waker = &to_static_waker(flags); //todo maybe inline it in struct cause this will run dummy drop
            func(handle,&mut Context::from_waker(waker),restart)
        };
        drop(guard); //explicit drop
        result
    }
}


struct StaticSyncFlags {
    global: &'static AtomicWakerRegistry,
    runnable: AtomicBool,
}

impl StaticSyncFlags {
    fn new(global: &'static AtomicWakerRegistry) -> Self {
        Self {
            global,
            runnable: AtomicBool::new(true),
        }
    }
    fn is_runnable(&self) -> bool { self.runnable.load(Ordering::Relaxed) }
    fn set_runnable(&self, value: bool) { self.runnable.store(value, Ordering::Release) }
}

impl DynamicWake for StaticSyncFlags {
    fn wake(&self) {
        self.runnable.store(true, Ordering::Release);
        self.global.notify_wake();
    }
}
