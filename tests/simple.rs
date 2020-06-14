use juggle::*;
use std::time::Duration;
use rand::Rng;


async fn waiting_task(handle: WheelHandle<'_>){
    println!("Wait Task [{}] enter",handle.get_current_name().as_deref().unwrap_or(""));
    for i in 1..20 {
        let dur = Duration::from_millis(rand::thread_rng().gen_range(10,20));
        smol::Timer::after(dur).await;
        println!("Handle: {:?}",&handle);
        println!("Wait Task [{}] point {}",handle.get_current_name().as_deref().unwrap_or(""),i);
    }
    smol::Timer::after(Duration::from_millis(3)).await;
    println!("Wait Task [{}] exit",handle.get_current_name().as_deref().unwrap_or(""));
}

async fn test_task(handle: WheelHandle<'_>){
    println!("Task [{}] enter",handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 1",handle.get_current_name().as_deref().unwrap_or(""));
    if handle.get_current_name().as_deref() == Some("T1") {
        handle.spawn(SpawnParams::named("WT10"), waiting_task(handle.clone()));
        handle.spawn(SpawnParams::named("WT11"), waiting_task(handle.clone()));
        handle.spawn(SpawnParams::named("WT12"), waiting_task(handle.clone()));
    }
    println!("Handle: {:#?}",&handle);
    yield_once!();
    println!("Task [{}] point 2",handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 3",handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] exit",handle.get_current_name().as_deref().unwrap_or(""));
}

#[test]
pub fn test_round_robin(){
    let sch = Wheel::new();
    let handle = sch.handle().clone();
    handle.spawn("T1", test_task(handle.clone()));
    handle.spawn("T2", test_task(handle.clone()));
    handle.spawn("T3", test_task(handle.clone()));
    handle.spawn("T4", test_task(handle.clone()));
    println!("Valid: {}",handle.is_valid());

    smol::run(sch).unwrap();
    println!("Valid: {}",handle.is_valid());
    println!("Finished round robin.");
}