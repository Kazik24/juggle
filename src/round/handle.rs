use std::rc::Weak;
use std::cell::UnsafeCell;
use crate::round::algorithm::SchedulerAlgorithm;

#[derive(Clone)]
pub struct WheelHandle{
    ptr: Weak<UnsafeCell<SchedulerAlgorithm>>,
}

impl WheelHandle{
    pub(crate) fn new(ptr: Weak<UnsafeCell<SchedulerAlgorithm>>)->Self{ Self{ptr} }
    pub fn is_valid(&self)->bool{ self.ptr.strong_count() != 0 }

}