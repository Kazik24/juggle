



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
            let pointer: unsafe fn(StaticHandle,&mut Context<'_>)->Poll<()> = |handle,cx|{
                //for now require Send + Sync + 'static, maybe it can be loosened to only
                //Send + 'static when scheduler will enforce it at runtime.
                type TaskType = impl Future<Output=()> + Send + Sync + 'static;
                fn wrapper(handle: StaticHandle)->TaskType{
                    $(let $handle_name = handle;)?
                    $async_expr
                }
                //todo check if this can cause unsafety cause operations aren't volatile
                static mut POLL: MaybeUninit<TaskType> = unsafe{ MaybeUninit::uninit()};
                static mut INIT_FLAG: u8 = 0; //uninit
                unsafe{
                    match INIT_FLAG {
                        0 =>{
                            POLL = MaybeUninit::new(wrapper(handle));
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
    use crate::static_round::wheel::StaticHandle;

    struct PtrWrapper(unsafe fn(StaticHandle,&mut Context<'_>)->Poll<()>);
    impl Future for PtrWrapper{
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            unsafe{ self.0(unsafe{MaybeUninit::uninit().assume_init()},cx) }
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
        let p1 = unsafe_static_poll_func!(()=>do_sth());
        let p2 = unsafe_static_poll_func!((name)=>do_sth_other());



        spin_block_on(PtrWrapper(p1));
        println!("**********************");
        spin_block_on(PtrWrapper(p2));
        println!("**********************");
        spin_block_on(PtrWrapper(p1));
        println!("**********************");
        spin_block_on(PtrWrapper(p2));
    }
}