
//! This crate provides a way to switch between tasks on single-thread environments without using
//! preemption.
//!





extern crate alloc;

mod round;
mod utils;
mod configure;
mod chunk_slab;


pub use self::utils::Yield;
pub use self::round::{Wheel, WheelHandle, LockedWheel};


#[macro_export]
macro_rules! yield_once{
    () => {
        $crate::Yield::once().await
    }
}