//! This crate provides a way to switch between tasks on single-thread or embedded environments utilizing
//! [cooperative multitasking](https://en.wikipedia.org/wiki/Cooperative_multitasking).
//!
//! It uses async/await mechanisms of Rust, tasks should have manual suspension points inside
//! async functions to allow scheduler to switch them (see [`yield_once!()`](macro.yield_once.html)
//! macro). Tasks can be dynamically [created]/[cancelled]/[suspended]/[resumed]
//! and can `await` external events (e.g from other threads or interrupts) as any normal async
//! functions. For more information about scheduler
//! see [`Wheel`](struct.Wheel.html).
//!
//! [created]: struct.WheelHandle.html#method.spawn
//! [cancelled]: struct.WheelHandle.html#method.cancel
//! [suspended]: struct.WheelHandle.html#method.suspend
//! [resumed]: struct.WheelHandle.html#method.resume
//! # Examples
//! Simple program that reads data from sensor and processes it.
//! ```
//! # extern crate alloc;
//! use juggle::*;
//! use alloc::collections::VecDeque;
//! use core::cell::RefCell;
//!
//! # use core::sync::atomic::*;
//! # async fn read_temperature_sensor()->i32 { 10 }
//! # fn init_timer(){}
//! # static CNT: AtomicUsize = AtomicUsize::new(0);
//! # fn get_timer_value()->u32 { CNT.fetch_add(1,Ordering::Relaxed) as _ }
//! # fn reset_timer(){CNT.store(0,Ordering::Relaxed);}
//! # fn shutdown_timer(){}
//! # fn process_data(queue: &mut VecDeque<i32>){
//! #     while let Some(_) = queue.pop_front() { }
//! # }
//! async fn collect_temperature(queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
//!     loop{ // loop forever or until cancelled
//!         let temperature: i32 = read_temperature_sensor().await;
//!         queue.borrow_mut().push_back(temperature);
//!         yield_once!(); // give scheduler opportunity to execute other tasks
//!     }
//! }
//!
//! async fn wait_for_timer(id: IdNum,queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
//!     init_timer();
//!     for _ in 0..5 {
//!         yield_while!(get_timer_value() < 200); // busy wait but also executes other tasks.
//!         process_data(&mut queue.borrow_mut());
//!         reset_timer();
//!     }
//!     handle.cancel(id); // cancel 'collect_temperature' task.
//!     shutdown_timer();
//! }
//!
//! fn main(){
//!     let queue = &RefCell::new(VecDeque::new());
//!     let wheel = Wheel::new();
//!     let handle = wheel.handle(); // handle to manage tasks, can be cloned inside this thread
//!
//!     let temp_id = handle.spawn(SpawnParams::default(),
//!                                collect_temperature(queue,handle.clone()));
//!     handle.spawn(SpawnParams::default(),
//!                  wait_for_timer(temp_id.unwrap(),queue,handle.clone()));
//!
//!     // execute tasks
//!     smol::block_on(wheel).unwrap(); // or any other utility to block on future.
//! }
//! ```


#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![cfg_attr(not(feature = "std"), no_std)]

#![warn(clippy::cargo,
    clippy::needless_borrow,
    clippy::pedantic,
    clippy::nursery)]

extern crate alloc;

pub mod utils;
mod round;
mod yield_helper;
mod block;

pub use self::block::{block_on, spin_block_on};
pub use self::round::{IdNum, LockedWheel, SpawnParams, State, SuspendError, Wheel, WheelHandle};
pub use self::yield_helper::{Yield, YieldTimes, YieldWhile};



/// Yield current task. Gives the scheduler opportunity to switch to another task.
/// Equivalent to [`Yield::once().await`](struct.Yield.html#method.once).
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
/// # fn main(){ smol::block_on(some_task()); }
/// ```
#[macro_export]
macro_rules! yield_once {
    () => {
        $crate::Yield::once().await
    }
}


/// Yield current task until specific expression becomes false.
/// Gives the scheduler opportunity to switch to another task.
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
///     yield_while!(get_timer_value() < 10);
///     do_some_work();
///     shutdown_external_timer();
/// }
/// # fn main(){ smol::block_on(timer_task()); }
/// ```
#[macro_export]
macro_rules! yield_while {
    ($test_expr:expr) => {
        $crate::Yield::yield_while(||{
            let ret: bool = $test_expr;
            ret
        }).await
    }
}
