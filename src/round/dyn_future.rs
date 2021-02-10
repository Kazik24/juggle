use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::{Cell, UnsafeCell};
use core::future::Future;
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::*;
use crate::utils::{AtomicWakerRegistry, DynamicWake, to_waker, DropGuard};
use crate::SpawnParams;
use crate::round::stat::{TaskWrapper, StopReason};

pub(crate) struct DynamicFuture<'a> {
    //not send not sync
    pinned_future: UnsafeCell<Pin<Box<dyn Future<Output=()> + 'a>>>,
    flags: SyncFlags,
    name: TaskName,
    stop_reason: Cell<StopReason>,
    polling: Cell<bool>,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub(crate) enum TaskName {
    Static(&'static str),
    Dynamic(Box<str>),
    None,
}
impl TaskName {
    pub fn as_str(&self)->Option<&str>{
        match self {
            TaskName::Static(s) => Some(s),
            TaskName::Dynamic(s) => Some(s.deref()),
            TaskName::None => None,
        }
    }
}

impl<'a> DynamicFuture<'a> {
    pub fn new(future: Pin<Box<dyn Future<Output=()> + 'a>>, global: Arc<AtomicWakerRegistry>,
               params: SpawnParams) -> Self {
        Self {
            pinned_future: UnsafeCell::new(future),
            flags: SyncFlags::new(global),
            name: params.name,
            stop_reason: Cell::new(if params.suspended {StopReason::Suspended} else {StopReason::None}),
            polling: Cell::new(false),
        }
    }
}

impl<'a> TaskWrapper for DynamicFuture<'a>{
    fn get_name(&self) -> &TaskName { &self.name }
    fn get_stop_reason(&self) -> StopReason { self.stop_reason.get() }
    fn set_stop_reason(&self, val: StopReason) { self.stop_reason.set(val); }
    fn is_runnable(&self) -> bool { self.flags.is_runnable() }
    fn poll_local(&self) -> Poll<()> {
        //SAFETY: guard against undefined behavior of borrowing UnsafeCell mutably twice.
        if self.polling.replace(true) {
            panic!("Recursive call to DynamicFuture::poll_local is not allowed.");
        }
        let guard = DropGuard::new(||self.polling.set(false)); //SAFETY: construct guard

        //store false cause if it became true before this operation then polling can be done
        //if it becomes true after this operation but before poll then this also means that polling can be done
        self.flags.set_runnable(false);

        //SAFETY: we just checked if this function was called recursively.
        let result = unsafe {
            let pin_ref = &mut *self.pinned_future.get();
            pin_ref.as_mut().poll(&mut Context::from_waker(self.flags.waker_ref()))
        };
        drop(guard); //explicit drop
        result
    }
}


struct SyncFlags {
    flags: Arc<InnerSyncFlags>,
    // inline waker cause we want to avoid cloning (optimally i would like this to be only field cause
    // this waker is actually wrapped arc from above, but you cant access inner content of waker
    // without using transmute, so i don't want to do that)
    waker: Waker,
}

impl SyncFlags {
    fn new(global: Arc<AtomicWakerRegistry>) -> Self {
        let flags = Arc::new(InnerSyncFlags {
            global,
            runnable: AtomicBool::new(true),
        });
        Self {
            waker: to_waker(flags.clone()),
            flags,
        }
    }
    fn waker_ref(&self) -> &Waker { &self.waker }
    fn is_runnable(&self) -> bool { self.flags.runnable.load(Ordering::Relaxed) }
    fn set_runnable(&self, value: bool) { self.flags.runnable.store(value, Ordering::Release) }
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