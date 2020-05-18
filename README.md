# juggle
Async task switching for single threaded environments.
<br>
`Work in progress...`

This library provides tools to manage a group of tasks in single threaded or embedded
environments. Note than this is not an operating system which means it's very cheap.
It uses async/await mechanisms of Rust. Tasks are scheduled non-preemptively, you can
insert manual preemption, create/cancel/suspend tasks using this library.

Example of using this library:
 ```rust
 use juggle::*;
 use alloc::collections::VecDeque;
 use alloc::rc::Rc;
 use core::cell::RefCell;

 async fn collect_temperature(queue: Rc<RefCell<VecDeque<i32>>>,handle: WheelHandle){
     loop{ // loop forever or until cancelled
         let temperature: i32 = read_temperature_sensor().await;
         queue.borrow_mut().push_back(temperature);
         yield_once!(); // give scheduler opportunity to execute other tasks
     }
 }

 async fn wait_for_timer(id: IdNum,queue: Rc<RefCell<VecDeque<i32>>>,handle: WheelHandle){
     init_timer();
     for _ in 0..5 {
         yield_until!(get_timer_value() >= 200); // busy wait but also executes other tasks.
         process_data(&mut queue.borrow_mut());
     }
     handle.cancel(id); // cancel 'collect_temperature' task.
 }

 fn main(){
     let wheel = Wheel::new();
     let handle = wheel.handle(); // handle to manage tasks, can be cloned inside this thread
     let queue = Rc::new(RefCell::new(VecDeque::new()));

     let temp_id = handle.spawn(SpawnParams::default(),
                                collect_temperature(queue.clone(),handle.clone()));
     handle.spawn(SpawnParams::default(),
                  wait_for_timer(temp_id.unwrap(),queue.clone(),handle.clone()));

     // execute tasks
     smol::block_on(wheel); // or any other utility to block on future.
 }
 ```