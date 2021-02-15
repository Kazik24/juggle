
use crate::st::stt_future::StaticFuture;
use crate::st::polling::*;
use crate::st::handle::StaticHandle;//todo macro
use crate::st::wheel::StaticWheelDef;
use crate::st::StaticParams;

#[macro_export]
macro_rules! clear_expr{
    ($e:expr) => {()}
}
#[macro_export]
macro_rules! count_expr{
    () => { 0 };
    ($($e:expr),+) => {
        [$(clear_expr!($e)),+].len()
    };
}

#[macro_export]
macro_rules! static_config {
    (
        $(
        ( $($handle_var:ident)? )
        $( $params_expr:expr )?
        => $async_expr:expr
        ),*
    ) => {
        {
            use $crate::st::{StaticHandle, StaticParams};
            use $crate::macro_private::*;
            static ARRAY: [StaticFuture;count_expr!($($async_expr),*)] = [$(
                StaticFuture::new(unsafe_static_poll_func!(($($handle_var)?)=>$async_expr),
                { StaticParams::new() $(; $params_expr )?})
                ),*];
            StaticWheelDef::from_raw_config(&ARRAY)
        }
    }
}

#[cfg(test)]
mod tests{
    #[macro_use]
    use super::*;
    use crate::{spin_block_on, Yield};
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Poll, Context};
    use std::mem::MaybeUninit;
    use crate::utils::{DropGuard, to_waker};
    use crate::st::wheel::StaticWheel;
    use crate::st::handle::StaticHandle;
    use std::cell::UnsafeCell;
    use crate::st::stt_future::StaticFuture;
    use crate::st::algorithm::StaticAlgorithm;
    use std::io::{stdout, Write};
    use std::sync::Arc;
    use std::sync::atomic::{Ordering, AtomicUsize};
    use crate::dy::SuspendError;
    use std::task::Waker;

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
        let wheel = CONFIG.lock();
        wheel.spin_block().unwrap();
    }
}