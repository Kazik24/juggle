use std::ptr::NonNull;
use core::future::Future;
use core::task::*;
use std::sync::Arc;
use crate::utils::{AtomicWakerRegistry, to_waker, DynamicWake};
use std::sync::atomic::{AtomicBool, Ordering};
use std::pin::Pin;

pub struct DynamicFuture{ //not send not sync
    pinned_future: NonNull<dyn Future<Output=()>>,
    flags: SyncFlags,
    name: Option<String>,
    suspended: bool,
}

impl DynamicFuture{
    //safe
    pub fn new_allocated(mut future: Pin<Box<dyn Future<Output=()>>>,
                         global: Arc<AtomicWakerRegistry>,suspended: bool)->Self{
        let ptr = unsafe{
            NonNull::new_unchecked(Box::into_raw(Pin::into_inner_unchecked(future)))
        };
        Self{
            pinned_future: ptr,
            flags: SyncFlags::new(false,global),
            name: None,
            suspended,
        }
    }
    //unsafe cause this future can be pinned as local variable on stack, and we erase its lifetime so
    //that that it need to be ensured that this object is not used after that variable gets dropped.
    pub unsafe fn new_static(future: Pin<&mut (dyn Future<Output=()> + 'static)>,
                             global: Arc<AtomicWakerRegistry>,suspended: bool)->Self{
        let ptr = unsafe{
            NonNull::new_unchecked(Pin::into_inner_unchecked(future) as *mut _)
        };
        Self{
            pinned_future: ptr,
            flags: SyncFlags::new(true,global),
            name: None,
            suspended,
        }
    }
    pub fn set_name(&mut self,name: impl Into<String>){self.name = Some(name.into())}
    pub fn get_name(&self)->&str{ self.name.as_deref().unwrap_or("") }
    pub fn set_suspended(&mut self,val: bool){self.suspended = val;}
    pub fn is_suspended(&self)->bool{self.suspended}
    pub fn is_runnable(&self)->bool{self.flags.is_runnable()}
    pub fn poll_local(&mut self)->Poll<()>{
        //store false cause if it became true before this operation then polling can be done
        //if it becomes true after this operation but before poll then this also means that polling can be done
        self.flags.set_runnable(false);
        let future = unsafe{ Pin::new_unchecked(self.pinned_future.as_mut()) };
        future.poll(&mut Context::from_waker(self.flags.waker_ref()))
    }
}
impl Drop for DynamicFuture{
    fn drop(&mut self) {
        if !self.flags.is_static() {
            unsafe {
                drop(Box::from_raw(self.pinned_future.as_ptr())); //no need to wrap in pin
            }
        }
    }
}


struct SyncFlags{
    flags: Arc<InnerSyncFlags>,
    waker: Waker, //inline waker cause we want to avoid cloning
}
impl SyncFlags{
    fn new(is_static: bool,global: Arc<AtomicWakerRegistry>)->Self{
        let flags = Arc::new(InnerSyncFlags{
            global,is_static,
            runnable: AtomicBool::new(true),
        });
        Self{
            waker: to_waker(flags.clone()),
            flags,
        }
    }
    fn waker_ref(&self)->&Waker{&self.waker}
    fn is_static(&self)->bool{self.flags.is_static}
    fn is_runnable(&self)->bool{self.flags.runnable.load(Ordering::Relaxed)}
    fn set_runnable(&self,value: bool){self.flags.runnable.store(value,Ordering::Release)}
}
struct InnerSyncFlags{
    global: Arc<AtomicWakerRegistry>,
    runnable: AtomicBool,
    is_static: bool,
}
impl DynamicWake for InnerSyncFlags{
    fn wake(&self) {
        self.runnable.store(true,Ordering::Release);
        self.global.notify_wake();
    }
}