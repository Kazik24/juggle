
use crate::st::{StaticParams, StaticWheelDef, StaticHandle};
use core::task::{Context, Poll};
use core::mem::MaybeUninit;
use core::future::Future;
use core::pin::Pin;

#[macro_export]
macro_rules! static_config {
    (@impl_clr $e:expr) => {()};
    (@impl_cnt ) => { 0 };
    (@impl_cnt $($e:expr),+) => {
        [$(static_config!(@impl_clr $e)),+].len()
    };
    (
        $(
            ( $($handle_var:ident)? )
            $( $params_expr:expr )?
            => $async_expr:expr
        ),*
    ) => {
        {
            use core::future::Future;
            use core::mem::MaybeUninit;
            use $crate::st::{StaticHandle, StaticParams};
            use $crate::macro_private::*;
            static ARRAY: [StaticFuture;static_config!(@impl_cnt $($async_expr),*)] = [$(
                StaticFuture::new({
                    type TaskType = impl Future<Output=()> + 'static;
                    fn wrapper(_handle: StaticHandle)->TaskType{
                        $(let $handle_var = _handle;)?
                        $async_expr
                    }
                    FnPtrWrapper(|handle,cx,status|{
                        //todo check if this can cause unsafety cause operations aren't volatile
                        static mut POLL: MaybeUninit<TaskType> = unsafe{ MaybeUninit::uninit() };
                        static mut INIT_FLAG: u8 = 0; //uninit
                        unsafe{
                            handle_task(&mut POLL,&mut INIT_FLAG,status,move||wrapper(handle),cx)
                        }
                    }) //return pointer wrapper from expression
                },{ StaticParams::new() $(; $params_expr )?}) ),*];
            $crate::st::StaticWheelDef::from_raw_config(&ARRAY)
        }
    }
}

pub(crate) const RESTART_TASK:u8 = 1;
pub(crate) const CANCEL_TASK:u8 = 2;
pub(crate) const UNINIT_TASK:u8 = 3;
const FLAG_UNINIT: u8 = 0;
const FLAG_CREATED: u8 = 1;
const FLAG_DROPPED: u8 = 2;

/// Implementations specific for static_config! macro. Do not use directly.
#[derive(Copy, Clone)]
pub struct FnPtrWrapper(pub unsafe fn(StaticHandle,&mut Context<'_>,u8)->Poll<()>);
impl FnPtrWrapper{
    pub(crate) unsafe fn call(self,handle: StaticHandle,cx: &mut Context<'_>,status: u8)->Poll<()>{
        self.0(handle,cx,status)
    }
}

/// Implementations specific for static_config! macro. Do not use directly.
pub unsafe fn handle_task<T,F>(task: &'static mut MaybeUninit<T>,flag: &'static mut u8,status: u8,
                               init: F,cx: &mut Context<'_>)->Poll<()>
    where T: Future<Output=()> + 'static, F: FnOnce()->T{
    if status != 0 {
        debug_assert!(status == CANCEL_TASK || status == RESTART_TASK || status == UNINIT_TASK);
        //drop anyways
        match *flag {
            FLAG_CREATED => {//if already initialized
                //temporary uninit/drop in case destructor unwinds
                *flag = if status == RESTART_TASK || status == UNINIT_TASK { FLAG_UNINIT } else { FLAG_DROPPED };
                task.as_mut_ptr().drop_in_place(); //drop previous value
            }
            FLAG_DROPPED if status == UNINIT_TASK => { *flag = FLAG_UNINIT }
            FLAG_UNINIT if status == CANCEL_TASK => { *flag = FLAG_DROPPED }
            _ => {}
        }
        if status == RESTART_TASK { //if should restart
            *task = MaybeUninit::new(init());
            //mark after creating task, so in case of panic propagation this will remain uninit
            *flag = FLAG_CREATED;
        } else { //if should only drop
            return Poll::Ready(()); //return now
        }
    }else{
        match *flag {
            FLAG_UNINIT =>{
                *task = MaybeUninit::new(init());
                //mark after creating task, so in case of panic propagation this will remain uninitialized
                *flag = FLAG_CREATED;
            }
            FLAG_DROPPED => return Poll::Ready(()),
            _ => {} //1 = initialized
        }
    }

    debug_assert_eq!(*flag,FLAG_CREATED);
    //statics are never moved
    let pin = Pin::new_unchecked(&mut *task.as_mut_ptr());
    let result = pin.poll(cx);
    if result.is_ready() {
        //Mark ready to avoid unsafety in case function is polled again.
        //Marking is done before drop in so when destruction panic propagates it won't cause
        //double drop on next call.
        *flag = FLAG_DROPPED;
        //static will never be used again so we can drop it
        task.as_mut_ptr().drop_in_place();
    }
    result
}

#[cfg(test)]
mod tests{
    extern crate std;
    use super::*;
    use crate::{*, utils::*, dy::*};
    use crate::st::{StaticWheelDef,StaticWheel,StaticHandle};
    use std::sync::atomic::*;
    use std::sync::Arc;
    use std::task::Waker;
    use std::prelude::v1::*;
    use std::*;


    async fn do_sth(){
        let _guard = DropGuard::new(||println!("do_sth guard dropped"));
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
    async fn do_sth_other(handle: StaticHandle){
        let guard = DropGuard::new(||println!("do_sth_other guard dropped"));
        println!("*do_sth_other start");
        Yield::once().await;
        println!("do_sth_other x2");
        for i in 0..20{
            Yield::once().await;
            if i == 3 {handle.restart(handle.get_id_by_index(0));}
            println!("do_sth_other {}",i);
        }
        Yield::once().await;
        println!("do_sth_other x3");
        Yield::once().await;
        println!("*do_sth_other end");
        drop(guard);
    }

    static CONFIG: StaticWheelDef = static_config!{
        () StaticParams::named("do_sth") => do_sth(),
        (handle)=>do_sth_other(handle)
    };
    struct UnderTest{
        wheel: Pin<Box<StaticWheel>>,
        count: Arc<AtomicUsize>,
        waker: Waker,
    }
    impl UnderTest{
        pub fn new(wheel: StaticWheel)->Self{
            let count = Arc::new(AtomicUsize::new(0));
            Self{
                wheel: Box::pin(wheel),
                count: count.clone(),
                waker: to_waker(Arc::new(move||{count.fetch_add(1,Ordering::SeqCst);})),
            }
        }
        pub fn poll_once(&mut self)->Poll<Result<(),SuspendError>>{
            self.wheel.as_mut().poll(&mut Context::from_waker(&self.waker))
        }
        pub fn wake_count(&self)->usize{ self.count.load(Ordering::SeqCst) }

    }
    #[test]
    fn test_config(){
        CONFIG.lock().spin_block().unwrap();
        CONFIG.lock();
    }
}