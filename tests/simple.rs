use juggle::*;
use futures::executor::block_on;
use std::time::Duration;
use std::collections::hash_map::RandomState;
use futures::{SinkExt, StreamExt};
use std::hash::{BuildHasher, Hasher};

async fn delay_thread(dur: Duration){
    use futures::channel::mpsc::channel;
    let (mut s,mut r) = channel(0);
    std::thread::spawn(move ||{
        std::thread::sleep(dur);
        block_on(s.send(()));
    });
    r.next().await;
}
async fn waiting_task(id:i32){
    println!("Wait Task [{}] enter",id);
    for i in 1..20 {
        let val = RandomState::new().build_hasher().finish();
        delay_thread(Duration::from_millis(1 + (val % (10 + id as u64)))).await;
        println!("Wait Task [{}] point {}",id,i);
    }
    delay_thread(Duration::from_millis(3)).await;
    println!("Wait Task [{}] exit",id);
}

async fn test_task(id:i32,handle: WheelHandle){
    println!("Task [{}] enter",id);
    yield_once!();
    println!("Task [{}] point 1",id);
    if id == 1 {
        handle.spawn_named("WT10",waiting_task(id*10));
        handle.spawn_named("WT11",waiting_task(id*10+1));
        handle.spawn_named("WT12",waiting_task(id*10+2));
    }
    yield_once!();
    println!("Task [{}] point 2",id);
    yield_once!();
    println!("Task [{}] point 3",id);
    yield_once!();
    println!("Task [{}] exit",id);
}

#[test]
pub fn test_round_robin(){
    let sch = Wheel::new();
    let handle = sch.handle();
    handle.spawn_named("T1",test_task(1,handle.clone()));
    handle.spawn_named("T2",test_task(2,handle.clone()));
    handle.spawn_named("T3",test_task(3,handle.clone()));
    handle.spawn_named("T4",test_task(4,handle.clone()));
    println!("Valid: {}",handle.is_valid());
    // let other = Wheel::new();
    // let safe = other.try_lock().ok().unwrap();
    // spawn(move ||{
    //     block_on(safe);
    // });

    block_on(sch);
    println!("Valid: {}",handle.is_valid());
    println!("Finished round robin.");
}