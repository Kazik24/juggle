use std::task::Poll;
use crate::round::dyn_future::TaskName;
use std::future::Future;
use std::pin::Pin;

pub trait TaskRegistry<T: TaskFuture>{ //generailize registry so that it can hold fixed or dynamic futures



}

#[repr(u8)]
#[derive(Copy,Clone,Eq,PartialEq,Hash,Debug)]
pub enum StopReason{ None,Suspended,Cancelled }
impl StopReason{
    pub fn is_poll_allowed(self)->bool{ self == Self::None }
}


pub trait TaskFuture{

    //fn get_name(&self) -> &TaskName;
    fn get_stop_reason(&self)->StopReason;
    fn set_stop_reason(&self,val: StopReason);
    fn is_runnable(&self) -> bool;
    fn poll_local(&self) -> Poll<()>;

}