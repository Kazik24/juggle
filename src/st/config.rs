
use crate::st::stt_future::StaticFuture;
use crate::st::pooling::*;
use crate::st::wheel::StaticHandle;//todo macro
use crate::st::wheel::StaticWheelDef;

macro_rules! static_config {
    ($var_name:ident : [$count:literal] $(seed: $seed:literal)? {
        $(
        ( $($handle_var:ident)? ) => $async_expr:expr
        ),*
    }) => {
        static $var_name: StaticWheelDef = {
            const fn make_array()->[StaticFuture;$count]{
                let mut array = [
                $(StaticFuture::new(unsafe_static_poll_func!(($($handle_var)?)=>$async_expr),None,false)),*
                ];
                //todo implement random shuffle by seed
                array
            }
            static ARRAY: [StaticFuture;$count] = make_array();
            StaticWheelDef::from_raw_config(&ARRAY)
        };
    }
}
async fn todo(){}
const fn make_array()->[StaticFuture;1]{
    let mut array = [StaticFuture::new(unsafe_static_poll_func!(()=>todo()),None,false)];
    array
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{spin_block_on, Yield};
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Poll, Context};
    use std::mem::MaybeUninit;
    use crate::utils::{DropGuard, to_waker};
    use crate::st::wheel::{StaticHandle, StaticWheel};
    use std::cell::UnsafeCell;
    use crate::st::stt_future::StaticFuture;
    use crate::st::algorithm::StaticAlgorithm;
    use std::io::{stdout, Write};
    use std::sync::Arc;
    use std::sync::atomic::{Ordering, AtomicUsize};
    use crate::dy::SuspendError;
    use std::task::Waker;

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
    async fn do_sth_other(handle: StaticHandle){
        let guard = DropGuard::new(||println!("do_sth_other guard dropped"));
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
        drop(guard);
    }

    static_config!{
        CONFIG: [2]{
            ()=>do_sth(),
            (handle)=>do_sth_other(handle)
        }
    }
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
        let wheel = CONFIG.lock();
        wheel.spin_block();
    }
}