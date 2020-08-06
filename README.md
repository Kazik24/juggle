# juggle

[![Build](https://github.com/Kazik24/juggle/workflows/Build%20and%20test/badge.svg)](
https://github.com/Kazik24/juggle/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](
https://github.com/Kazik24/juggle)

Async task switching for single threaded environments.
<br>
`Work in progress...`

This library provides tools to manage a group of tasks in single threaded or embedded
environments. Note than this is not an operating system which means it's very cheap.
It uses async/await mechanisms of Rust. Tasks are scheduled non-preemptively, which
means they are not executed concurrently, you can insert manual switching points
using provided macros or `Yield` utility. Tasks can be dynamically created, cancelled,
suspended or resumed. If all tasks are currently waiting to be woken then scheduler
itself will yield.

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
        yield_until!(get_timer_value() >= 200); // busy wait but also executes other tasks.
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