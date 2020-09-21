# juggle

[![Build](https://github.com/Kazik24/juggle/workflows/Build%20and%20test/badge.svg)](
https://github.com/Kazik24/juggle/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](
https://github.com/Kazik24/juggle)
[![Crate](https://img.shields.io/crates/v/juggle.svg)](
https://crates.io/crates/juggle)
[![Documentation](https://docs.rs/juggle/badge.svg)](
https://docs.rs/juggle)
[![Rust version](https://img.shields.io/badge/Rust-1.46+-blueviolet.svg)](
https://www.rust-lang.org)

Async task switching for
[cooperative multitasking](https://en.wikipedia.org/wiki/Cooperative_multitasking)
in single thread environments with `no_std` support.

This library provides tools to dynamically manage group of tasks in single threaded or embedded
environments. Note than this is not an operating system but can serve as a simple replacement
where you don't need context switches, for example on embedded applications.
Tasks use async/await mechanisms of Rust and it's programmer job to insert switching
points into them. Luckily this crate provides `Yield` utility for this as well as
handling busy waits. Primary scheduler (`Wheel`) can dynamically spawn, suspend, resume
or cancel tasks managed by round robin algorithm.

Crate by default uses std library (feature `std`) but also can be configured
as `#![no_std]` with `alloc` crate, this disables some features in `utils`
module.

### Examples
Simple program that reads data from sensor and processes it.
```rust
use juggle::*;
use alloc::collections::VecDeque;
use core::cell::RefCell;

async fn collect_temperature(queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
    loop{ // loop forever or until cancelled
        let temperature: i32 = read_temperature_sensor().await;
        queue.borrow_mut().push_back(temperature);
        yield_once!(); // give scheduler opportunity to execute other tasks
    }
}

async fn wait_for_timer(id: IdNum,queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
    init_timer();
    for _ in 0..5 {
        yield_while!(get_timer_value() < 200); // busy wait but also executes other tasks.
        process_data(&mut queue.borrow_mut());
        reset_timer();
    }
    handle.cancel(id); // cancel 'collect_temperature' task.
    shutdown_timer();
}

fn main(){
    let queue = &RefCell::new(VecDeque::new());
    let wheel = Wheel::new();
    let handle = wheel.handle(); // handle to manage tasks, can be cloned inside this thread

    let temp_id = handle.spawn(SpawnParams::default(),
                               collect_temperature(queue,handle.clone()));
    handle.spawn(SpawnParams::default(),
                 wait_for_timer(temp_id.unwrap(),queue,handle.clone()));

    // execute tasks
    smol::block_on(wheel).unwrap(); // or any other utility to block on future.
}
 ```