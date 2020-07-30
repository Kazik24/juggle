
//! This crate provides a way to switch between tasks on single-thread environments without using
//! preemption.
//!

#![warn(missing_docs)]
//#![no_std]

extern crate alloc;

pub mod utils;
mod round;
mod chunk_slab;
mod yield_helper;
mod block;

pub use self::yield_helper::{Yield, YieldTimes, YieldUntil};
pub use self::round::{Wheel, WheelHandle, LockedWheel, IdNum, SpawnParams, State, SuspendError};
pub use self::block::spin_block_on;


/// Yield current task. Gives the sheduler opportunity to switch to another task.
///
/// # Examples
///```
/// # #[macro_use]
/// # extern crate juggle;
/// # fn do_some_work(){}
/// # fn do_more_work(){}
/// # fn do_even_more_work(){}
/// async fn some_task(){
///     do_some_work();
///     yield_once!();
///     do_more_work();
///     yield_once!();
///     do_even_more_work();
/// }
/// # fn main(){ smol::run(some_task()); }
/// ```
#[macro_export]
macro_rules! yield_once{
    () => {
        $crate::Yield::once().await
    }
}


/// Yield current task until specific expression becomes true.
/// Gives the sheduler opportunity to switch to another task.
///
/// It is recommended to use this function instead of busy wait inside tasks within scheduler.
///
/// # Examples
///```
/// # #[macro_use]
/// # extern crate juggle;
/// # fn init_external_timer(){}
/// # fn get_timer_value()->u32{ 20 }
/// # fn shutdown_external_timer(){ }
/// # fn do_some_work(){}
/// async fn timer_task(){
///     init_external_timer();
///     yield_until!(get_timer_value() > 10);
///     do_some_work();
///     shutdown_external_timer();
/// }
/// # fn main(){ smol::run(timer_task()); }
/// ```
#[macro_export]
macro_rules! yield_until{
    ($test_expr:expr) => {
        $crate::Yield::until(||{
            let ret: bool = $test_expr;
            ret
        }).await
    }
}


#[cfg(test)]
pub(crate) mod test_util{
    extern crate std;
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::marker::PhantomData;
    use std::sync::{Mutex, Condvar, Arc};
    use std::collections::{HashMap, HashSet};
    use std::mem::replace;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{spawn, sleep};
    use std::time::Duration;
    use std::prelude::v1::*;

    std::thread_local!{
        static BREAK_CONTEXT: Cell<Option<NonNull<Breakpoints>>> = Cell::new(None);
    }
    macro_rules! test_break{
        ($id:literal) => {
            #[cfg(test)]
            $crate::test_util::Breakpoints::on_breakpoint($id);
        }
    }

    pub struct Breakpoints{
        mutex: Mutex<HashMap<u32,(bool,Arc<Condvar>)>>
    }
    pub struct BreakGuard<'a>(Option<NonNull<Breakpoints>>,PhantomData<&'a ()>);
    impl<'a> Drop for BreakGuard<'a>{
        fn drop(&mut self) { BREAK_CONTEXT.with(|b|b.set(self.0)); }
    }

    impl Breakpoints{
        pub fn new()->Self{
            Self{
                mutex: Mutex::new(HashMap::new()),
            }
        }
        pub fn register(&self)->BreakGuard{
            let next = unsafe{NonNull::new_unchecked(self as *const _ as *mut _)};
            let prev = BREAK_CONTEXT.with(|b|b.replace(Some(next)));
            BreakGuard(prev,PhantomData)
        }
        pub fn set_breakpoint(&self,id: u32,value: bool){
            let mut guard = self.mutex.lock().unwrap();
            let r = guard.entry(id).or_insert((false,Arc::new(Condvar::new())));
            if replace(&mut r.0,value) {
                if !value {
                    r.1.notify_all();
                }
            }
        }
        pub fn on_breakpoint(id: u32){
            let ctx = BREAK_CONTEXT.with(|b|b.get());
            if let Some(ctx) = ctx {
                let ctx = unsafe{&*ctx.as_ptr()};
                let guard = ctx.mutex.lock().unwrap();
                let var = match guard.get(&id) {
                    Some(var) => (var.0,var.1.clone()),
                    None => return,
                };
                if var.0 {
                    drop(var.1.wait(guard).unwrap());
                }
            }
        }
        pub fn get_held(&self)->Vec<u32>{
            let guard = self.mutex.lock().unwrap();
            guard.iter().filter_map(|(&k,(flag,_))| if *flag {Some(k)} else {None}).collect()
        }
    }

    #[test]
    fn test_breakpoints(){
        let brx = Arc::new(Breakpoints::new());
        let bry = Arc::new(Breakpoints::new());
        let m = Arc::new([AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)]);

        for i in 0..3 {
            brx.set_breakpoint(i,true);
            bry.set_breakpoint(i,true);
        }

        let br1 = brx.clone();
        let br2 = bry.clone();
        let marks = m.clone();
        let h = spawn(move||{
            let _guard = br1.register();
            test_break!(0);
            assert!(marks[0].load(Ordering::SeqCst));
            br2.set_breakpoint(0,false);

            test_break!(1);
            sleep(Duration::from_millis(10));
            assert!(marks[1].load(Ordering::SeqCst));
            br2.set_breakpoint(1,false);

            test_break!(2);
            sleep(Duration::from_millis(10));
            assert!(marks[2].load(Ordering::SeqCst));
            br2.set_breakpoint(2,false);
        });
        let _guard = bry.register();
        sleep(Duration::from_millis(100));

        let check = alloc::vec![0,1,2].into_iter().collect::<HashSet<_>>();
        assert_eq!(check,brx.get_held().into_iter().collect());
        assert_eq!(check,bry.get_held().into_iter().collect());

        m[0].store(true,Ordering::SeqCst);
        brx.set_breakpoint(0,false);
        test_break!(0);
        sleep(Duration::from_millis(10));
        m[1].store(true,Ordering::SeqCst);
        brx.set_breakpoint(1,false);
        test_break!(1);
        sleep(Duration::from_millis(10));
        m[2].store(true,Ordering::SeqCst);
        brx.set_breakpoint(2,false);
        test_break!(2);

        h.join().unwrap();
    }
}