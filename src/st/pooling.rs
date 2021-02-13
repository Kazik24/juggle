
pub(crate) const RESTART_TASK:u8 = 1;
pub(crate) const CANCEL_TASK:u8 = 2;


/// Creates function pointer (static) from given async expression.
/// Async expression is assigned to static variables controlling its state.
/// Calling function pointer with given context results in calling poll on underlined async expression.
/// Note that created function pointer is unsynchronized and thus marked unsafe, when calling, user
/// need to make sure that s|he is only one who calls it at this time of program. So wrap it in lock
/// or sth.
macro_rules! unsafe_static_poll_func{
    ( ( $($handle_name:ident )? ) => $async_expr:expr) => {
        {
            use core::future::Future;
            use core::pin::Pin;
            use core::mem::MaybeUninit;
            use core::task::{Poll, Context};

            type TaskType = impl Future<Output=()> + 'static;
            fn wrapper(_handle: StaticHandle)->TaskType{
                $(let $handle_name = _handle;)?
                $async_expr
            }
            let pointer: unsafe fn(StaticHandle,&mut Context<'_>,u8)->Poll<()> = |handle,cx,status|{
                //todo check if this can cause unsafety cause operations aren't volatile
                static mut POLL: MaybeUninit<TaskType> = unsafe{ MaybeUninit::uninit()};
                static mut INIT_FLAG: u8 = 0; //uninit
                unsafe{
                    if status != 0 {
                        //drop anyways
                        if INIT_FLAG == 1 {//if already initialized
                            //temporary uninit/drop in case destructor unwinds
                            INIT_FLAG = if status == 1 { 0 } else { 2 };
                            POLL.as_mut_ptr().drop_in_place(); //drop previous value
                        }
                        if status == 1 { //if should restart
                            POLL = MaybeUninit::new(wrapper(handle));
                            //mark after creating task, so in case of panic propagation this will remain uninit
                            INIT_FLAG = 1;
                        } else { //if should only drop
                            return Poll::Ready(()); //return now
                        }
                    }else{
                        match INIT_FLAG {
                            0 =>{
                                POLL = MaybeUninit::new(wrapper(handle));
                                //mark after creating task, so in case of panic propagation this will remain uninitialized
                                INIT_FLAG = 1;
                            }
                            2 => return Poll::Ready(()),
                            _ => {} //1 = initialized
                        }
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
        };
    };
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{spin_block_on, Yield};
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Poll, Context};
    use std::mem::MaybeUninit;
    use crate::utils::DropGuard;
    use crate::st::wheel::StaticHandle;
    use std::cell::UnsafeCell;
    use crate::st::stt_future::StaticFuture;
    use crate::st::algorithm::StaticAlgorithm;

    //todo this is only temporary prove of concept code, I know it has unsafe
    struct PtrWrapper<F>(unsafe fn(StaticHandle,&mut Context<'_>,u8)->Poll<()>,F);
    impl<F: FnMut()->u8> Future for PtrWrapper<F>{
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            unsafe{
                let flag = self.as_mut().get_unchecked_mut().1();
                self.0(MaybeUninit::uninit().assume_init(),cx,flag)
            }
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


    #[test]
    fn test_decl(){
        let p1 = unsafe_static_poll_func!(()=>do_sth());
        let p2 = unsafe_static_poll_func!((name)=>do_sth_other(name));
        let mut count = 0;


        spin_block_on(PtrWrapper(p1,||0));
        println!("**********************");
        spin_block_on(PtrWrapper(p2,||{
            count += 1;
            if count == 10 {
                println!("Restarting...");
                RESTART_TASK
            }else{ 0 }
        }));
        println!("**********************");
        spin_block_on(PtrWrapper(p1,||0));
        println!("**********************");
        spin_block_on(PtrWrapper(p2,||0));
    }
}