use std::future::Future;
use crate::SuspendError;
use std::marker::PhantomData;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::convert::identity;
use std::ops::Not;
use std::thread::spawn;

type StaticAlgorithm = crate::static_round::algorithm::StaticAlgorithm;

#[derive(Copy,Clone)]
#[repr(transparent)]
pub struct StaticHandle{
    alg: &'static StaticWheelDef,
    _phantom: PhantomData<*mut ()>,
}


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
    pub fn lock(&'static self)->StaticWheel{
        if !self.lock.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).unwrap_or_else(identity) {
            self.algorithm.init();
            return StaticWheel{ alg: &self, _phantom: PhantomData};
        }
        panic!("StaticWheel is already used elsewhere.");
    }
    pub unsafe fn get_unchecked(&'static self)->StaticWheel{ StaticWheel{ alg: &self, _phantom: PhantomData} }
    pub unsafe fn unlock(&'static self){
        self.lock.store(false,Ordering::Release);
    }
}


impl StaticWheel{
    pub fn handle(&self)->StaticHandle{
        StaticHandle{alg:self.alg,_phantom: PhantomData}
    }

}

#[cfg(test)]
mod tests{
    use super::*;
    use std::mem::MaybeUninit;
    use crate::static_round::stt_future::StaticFuture;

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
