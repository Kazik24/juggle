use core::task::Poll;
use crate::dy::dyn_future::TaskName;
use crate::dy::registry::{Registry, BorrowRef};

pub(crate) trait TaskRegistry<K: Copy>{ //generalize registry so that it can hold fixed or dynamic futures
    type Task;
    fn get(&self,key: K)->Option<BorrowRef<Self::Task>>;
    fn insert(&self,val: Self::Task)->Option<K>;
    fn remove(&self,key: K)->Option<()>;
    fn count(&self)->usize;
    fn capacity(&self)->usize;
}

#[repr(u8)]
#[derive(Copy,Clone,Eq,PartialEq,Hash,Debug)]
pub(crate) enum StopReason{ None,Suspended,Cancelled }
impl StopReason{
    pub fn is_poll_allowed(self)->bool{ self == Self::None }
}


pub(crate) trait TaskWrapper {

    fn get_name(&self) -> &TaskName;
    fn get_stop_reason(&self)->StopReason;
    fn set_stop_reason(&self,val: StopReason);
    fn is_runnable(&self) -> bool;
    fn poll_local(&self) -> Poll<()>;

}

pub(crate) trait BaseTaskRegistry{

}