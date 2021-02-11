use std::future::Future;
use crate::SuspendError;
use std::marker::PhantomData;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::convert::identity;
use std::ops::Not;
use std::thread::spawn;

type StaticAlgorithm = ();

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
    pub fn try_take(&'static self)->Option<StaticWheel>{
        if !self.lock.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).unwrap_or_else(identity) {
            return Some(StaticWheel{ alg: &self, _phantom: PhantomData});
        }
        None
    }
    pub fn take(&'static self)->StaticWheel{self.try_take().expect("StaticWheel is already used elsewhere.")}
}


impl StaticWheel{
    pub fn handle(&self)->StaticHandle{
        StaticHandle{alg:self.alg,_phantom: PhantomData}
    }

}
impl Drop for StaticWheel{
    fn drop(&mut self) {
        self.alg.lock.store(false,Ordering::Release);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn _test_sth(){
        static WHEEL: StaticWheelDef = StaticWheelDef{lock: AtomicBool::new(false),algorithm: ()};

        spawn(||{
            let w = WHEEL.take().handle(); //todo this immediately drops future, maybe take should lock wheel forever and only unsafe method would unlock it
            w.clone();
        });

    }
}
