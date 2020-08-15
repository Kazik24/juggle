//! This crate provides a way to switch between tasks on single-thread environments without using
//! preemption.
//!

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod utils;
mod round;
mod chunk_slab;
mod yield_helper;
mod block;

pub use self::block::{block_on, spin_block_on};
pub use self::round::{IdNum, LockedWheel, SpawnParams, State, SuspendError, Wheel, WheelHandle};
pub use self::yield_helper::{Yield, YieldTimes, YieldUntil};



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
macro_rules! yield_once {
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
macro_rules! yield_until {
    ($test_expr:expr) => {
        $crate::Yield::until(||{
            let ret: bool = $test_expr;
            ret
        }).await
    }
}
