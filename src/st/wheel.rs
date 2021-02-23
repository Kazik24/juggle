use std::future::Future;
use crate::dy::SuspendError;
use std::marker::PhantomData;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::convert::identity;
use std::ops::Not;
use std::thread::spawn;
use std::task::{Context, Poll};
use std::pin::Pin;
use crate::spin_block_on;
use crate::st::stt_future::StaticFuture;
use crate::st::handle::StaticHandle;
use crate::utils::DropGuard;

type StaticAlgorithm = crate::st::algorithm::StaticAlgorithm;

pub struct StaticWheelDef{
    lock: AtomicBool,
    algorithm: StaticAlgorithm,
}
unsafe impl Send for StaticWheelDef {}
unsafe impl Sync for StaticWheelDef {}

#[repr(transparent)]
pub struct StaticWheel{
    alg: &'static StaticWheelDef,
    _phantom: PhantomData<*mut ()>,
}

impl StaticWheelDef{
    /// This method should not be used, it is exposed only due to use in macro.
    pub const fn from_raw_config(config: &'static [StaticFuture])->Self{
        Self{
            lock: AtomicBool::new(false),
            algorithm: StaticAlgorithm::from_raw_config(config),
        }
    }
    pub fn is_locked(&'static self)->bool { self.lock.load(Ordering::Relaxed) }
    pub fn try_lock(&'static self)->Option<StaticWheel>{
        if !self.lock.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).unwrap_or_else(identity) {
            self.algorithm.init();
            return Some(StaticWheel{ alg: &self, _phantom: PhantomData});
        }
        None
    }
    pub fn lock(&'static self)->StaticWheel{ self.try_lock().expect("StaticWheel is already used elsewhere.") }

}


impl StaticWheel{
    pub fn handle(&self)->StaticHandle{
        //todo what to do with handle, it can survive StaticWheel drop
        StaticHandle{alg:&self.alg.algorithm,_phantom: PhantomData}
    }

    pub fn spin_block(self)->Result<(),SuspendError>{ spin_block_on(self) }
    pub fn spin_block_forever(self)->!{
        self.spin_block().unwrap();
        panic!("StaticWheel::spin_block_forever(): Didn't expect all tasks to finish.")
    }
}
impl Future for StaticWheel{
    type Output = Result<(),SuspendError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.alg.algorithm.poll_internal(cx).map(|flag| if flag { Ok(()) } else { Err(SuspendError) })
    }
}
impl Drop for StaticWheel{
    fn drop(&mut self) {
        let guard = DropGuard::new(||self.alg.lock.store(false,Ordering::Release));
        self.alg.algorithm.reset_all_tasks();
        drop(guard);
    }
}
