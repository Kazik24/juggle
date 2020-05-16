
//! This crate provides a way to switch between tasks on single-thread environments without using
//! preemption.
//!





extern crate alloc;

mod round;
mod utils;
mod configure;
mod chunk_slab;
mod yield_helper;


pub use self::yield_helper::Yield;
pub use self::round::{Wheel, WheelHandle, LockedWheel, TaskId, TaskParams};

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