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
    pub const fn from_raw_config(config: &'static [StaticFuture])->Self{
        Self{
            lock: AtomicBool::new(false),
            algorithm: StaticAlgorithm::from_raw_config(config),
        }
    }
    pub fn lock(&'static self)->StaticWheel{
        if !self.lock.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).unwrap_or_else(identity) {
            self.algorithm.init();
            return StaticWheel{ alg: &self, _phantom: PhantomData};
        }
        panic!("StaticWheel is already used elsewhere.");
    }
    pub unsafe fn get_unchecked(&'static self)->StaticWheel{
        if !self.lock.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).unwrap_or_else(identity) {
            self.algorithm.init();
        }
        StaticWheel{ alg: &self, _phantom: PhantomData}
    }
    pub unsafe fn unlock(&'static self){
        self.lock.store(false,Ordering::Release);
    }
}


impl StaticWheel{
    pub fn handle(&self)->StaticHandle{
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

#[cfg(test)]
mod tests{
    use super::*;
    use std::mem::MaybeUninit;
    use crate::st::stt_future::StaticFuture;

    fn _test_sth(){
        let wheel = Box::new(StaticWheelDef{lock: AtomicBool::new(false),algorithm:
            unsafe{MaybeUninit::zeroed().assume_init()}//todo remove
        });
        let wheel: &'static _ = Box::leak(wheel);

        spawn(move ||{
            let w = wheel.lock().handle(); //todo this immediately drops future, maybe take should lock wheel forever and only unsafe method would unlock it
            w.clone();
        });

    }
}
