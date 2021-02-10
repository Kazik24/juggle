use std::task::{Poll, Context};
use crate::round::dyn_future::{TaskName, DynamicFuture};
use std::future::Future;
use std::pin::Pin;
use std::ops::Deref;
use crate::round::registry::{Registry, BorrowRef};
use crate::Yield;
use std::mem::{size_of, MaybeUninit, align_of};

pub(crate) trait TaskRegistry<K: Copy>{ //generailize registry so that it can hold fixed or dynamic futures
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

macro_rules! make_config {
    ($name:ident,$($task:expr),*) =>{

    }
}



macro_rules! unsafe_static_poll_func{
    ($async_expr:expr) => {
        {
            let pointer: unsafe fn(&mut Context<'_>)->Poll<()> = |cx|{
                type TaskType = impl Future<Output=()>;
                fn wrapper()->TaskType{
                    $async_expr
                }
                static mut POLL: MaybeUninit<TaskType> = unsafe{ MaybeUninit::uninit()};
                static mut INIT_FLAG: u8 = 0; //uninit
                unsafe{
                    match INIT_FLAG {
                        0 =>{
                            POLL = MaybeUninit::new(wrapper());
                            //mark after creating task, so in case of panic propagation this will remain uninitialized
                            INIT_FLAG = 1;
                        }
                        2 => return Poll::Ready(()),
                        _ => {}
                    }
                    //statics are never moved
                    let pin = Pin::new_unchecked(&mut *POLL.as_mut_ptr());
                    let result = pin.poll(cx);
                    if result.is_ready() {
                        //Mark ready to avoid unsafety in case someone irresponsible calls this again.
                        //Marking is done before drop in so when destruction panic propagates it won't cause
                        //double drop on next irresponsible call.
                        INIT_FLAG = 2;
                        //static will never be used again so we can drop it
                        POLL.as_mut_ptr().drop_in_place();
                    }
                    result
                }
            };
            pointer //return pointer from expression
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::spin_block_on;

    struct PtrWrapper(unsafe fn(&mut Context<'_>)->Poll<()>);
    impl Future for PtrWrapper{
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            unsafe{ self.0(cx) }
        }
    }

    async fn do_sth(){
        println!("*do_sth start");
        Yield::once().await;
        println!("do_sth x2");
        for i in 0..10{
            Yield::once().await;
            println!("do_sth {}",i);
        }
        Yield::once().await;
        println!("*do_sth end");
    }
    async fn do_sth_other(){
        println!("*do_sth_other start");
        Yield::once().await;
        println!("do_sth_other x2");
        for i in 0..20{
            Yield::once().await;
            println!("do_sth_other {}",i);
        }
        Yield::once().await;
        println!("do_sth_other x3");
        Yield::once().await;
        println!("*do_sth_other end");
    }

    #[test]
    fn test_decl(){
        let p1 = unsafe_static_poll_func!(do_sth());
        let p2 = unsafe_static_poll_func!(do_sth_other());



        spin_block_on(PtrWrapper(p1));
        println!("**********************");
        spin_block_on(PtrWrapper(p2));
        println!("**********************");
        spin_block_on(PtrWrapper(p1));
        println!("**********************");
        spin_block_on(PtrWrapper(p2));
    }
}



