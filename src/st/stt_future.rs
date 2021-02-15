use std::cell::{Cell, UnsafeCell};
use crate::dy::stat::TaskWrapper;
use crate::dy::dyn_future::TaskName;
use std::task::{Context, Poll, Waker};
use crate::utils::{DropGuard, AtomicWakerRegistry, DynamicWake, to_waker, to_static_waker, noop_waker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crate::st::handle::StaticHandle;
use std::mem::MaybeUninit;
use crate::st::polling::{CANCEL_TASK, RESTART_TASK};
use crate::st::{StopReason, StaticParams};

pub struct StaticFuture{
    //not send not sync
    static_poll: unsafe fn(StaticHandle,&mut Context<'_>,u8) ->Poll<()>,
    flags: UnsafeCell<Option<StaticSyncFlags>>,
    name: Option<&'static str>,
    stop_reason: Cell<StopReason>,
    polling: Cell<bool>,
}

//this is fake for StaticFuture alone, but allows to hold it in statics
unsafe impl Send for StaticFuture {}
unsafe impl Sync for StaticFuture {}


impl StaticFuture{
    pub const fn new(poll: unsafe fn(StaticHandle,&mut Context<'_>,u8) ->Poll<()>,
                     params: StaticParams)->Self{
        Self{
            static_poll: poll,
            flags: UnsafeCell::new(None),
            name: params.name,
            stop_reason: Cell::new(if params.suspended {StopReason::Suspended} else {StopReason::None}),
            polling: Cell::new(false),
        }
    }
    pub(crate)fn init(&self,global: &'static AtomicWakerRegistry){
        let flags = unsafe{ &mut *self.flags.get() };
        *flags = Some(StaticSyncFlags::new(global));
    }
    fn get_flags(&self)->&StaticSyncFlags{
        unsafe{ &*self.flags.get() }.as_ref().expect("StaticFuture init error")
    }

    pub(crate)fn get_name(&self) -> Option<&str> { self.name }
    pub(crate)fn get_stop_reason(&self) -> StopReason { self.stop_reason.get() }
    pub(crate)fn set_stop_reason(&self, val: StopReason) { self.stop_reason.set(val); }
    pub(crate)fn is_runnable(&self) -> bool { self.get_flags().is_runnable() }
    pub(crate)fn cancel(&self,handle: StaticHandle){
        self.with_polling(move||{
            //SAFETY: we call this inside with_polling.
            unsafe {
                let func = self.static_poll;
                let res = func(handle,&mut Context::from_waker(&noop_waker()),CANCEL_TASK);
                debug_assert!(res.is_ready());
            }
        })
    }
    fn with_polling<T>(&self,func: impl FnOnce()->T)->T{
        //SAFETY: guard against undefined behavior of recursive polling.
        if self.polling.replace(true) {
            panic!("Recursive call to StaticFuture::poll_local is not allowed.");
        }
        let guard = DropGuard::new(||self.polling.set(false)); //SAFETY: construct guard
        let result = func();
        drop(guard); //explicit drop
        result
    }
    pub(crate)fn poll_local(&'static self,handle: StaticHandle,restart: bool) -> Poll<()> {
        self.with_polling(move||{
            //store false cause if it became true before this operation then polling can be done
            //if it becomes true after this operation but before poll then this also means that polling can be done
            let flags = self.get_flags();
            flags.set_runnable(false);

            //SAFETY: we call this inside with_polling.
            unsafe {
                let func = self.static_poll;
                let waker = &to_static_waker(flags); //todo maybe inline it in struct cause this will run dummy drop
                func(handle,&mut Context::from_waker(waker),if restart { RESTART_TASK } else { 0 })
            }
        })
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
