use core::future::Future;
use crate::dy::SuspendError;
use core::marker::PhantomData;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};
use core::convert::identity;
use core::ops::Not;
use core::task::{Context, Poll};
use core::pin::Pin;
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
            //created earlier just in case init panic
            let obj = StaticWheel{ alg: &self, _phantom: PhantomData};
            self.algorithm.init();
            return Some(obj);
        }
        None
    }
    pub fn lock(&'static self)->StaticWheel{ self.try_lock().expect("StaticWheel is already used elsewhere.") }

}


impl StaticWheel{
    pub fn handle(&self)->StaticHandle{
        //todo what to do with handle, it can survive StaticWheel drop
        let alg = &self.alg.algorithm;
        StaticHandle{alg,_phantom: PhantomData,generation_id: alg.get_generation()}
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
        //we need to properly dispose scheduler, only after that lock can be released
        self.alg.algorithm.dispose();
        drop(guard);
    }
}
