use std::time::Duration;
use rand::Rng;
use juggle::dy::*;
use juggle::*;

async fn waiting_task(handle: WheelHandle<'_>) {
    println!("Wait Task [{}] enter", handle.get_current_name().as_deref().unwrap_or(""));
    for i in 1..20 {
        let dur = Duration::from_millis(rand::thread_rng().gen_range(10, 20));
        smol::Timer::after(dur).await;
        println!("Handle: {:?}", &handle);
        println!("Wait Task [{}] point {}", handle.get_current_name().as_deref().unwrap_or(""), i);
    }
    smol::Timer::after(Duration::from_millis(3)).await;
    println!("Wait Task [{}] exit", handle.get_current_name().as_deref().unwrap_or(""));
}

async fn test_task(handle: WheelHandle<'_>) {
    println!("Task [{}] enter", handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 1", handle.get_current_name().as_deref().unwrap_or(""));
    if handle.get_current_name().as_deref() == Some("T1") {
        handle.spawn(SpawnParams::named("WT10"), waiting_task(handle.clone())).unwrap();
        handle.spawn(SpawnParams::named("WT11"), waiting_task(handle.clone())).unwrap();
        handle.spawn(SpawnParams::named("WT12"), waiting_task(handle.clone())).unwrap();
    }
    println!("Handle: {:#?}", &handle);
    yield_once!();
    println!("Task [{}] point 2", handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 3", handle.get_current_name().as_deref().unwrap_or(""));
    yield_once!();
    println!("Task [{}] exit", handle.get_current_name().as_deref().unwrap_or(""));
}

#[test]
pub fn test_round_robin() {
    let sch = Wheel::new();
    let handle = sch.handle().clone();
    handle.spawn("T1", test_task(handle.clone())).unwrap();
    handle.spawn("T2", test_task(handle.clone())).unwrap();
    handle.spawn("T3", test_task(handle.clone())).unwrap();
    handle.spawn("T4", test_task(handle.clone())).unwrap();
    println!("Valid: {}", handle.is_valid());

    smol::block_on(sch).unwrap();
    println!("Valid: {}", handle.is_valid());
    println!("Finished round robin.");
}

#[test]
fn test_drop_with_name() {
    let wheel = Wheel::new();
    let handle = wheel.handle().clone();
    let id = handle.spawn("some name", async move {}).unwrap();
    handle.with_name(id, |str| {
        drop(wheel);//drop wheel
        assert!(handle.is_valid());//handle must be valid because with_name didn't finish execution
        assert_eq!(str, Some("some name"));
    });
    assert!(!handle.is_valid());//now it should be invalidated.
}

#[test]
#[should_panic]
fn test_lock_with_name() {
    let wheel = Wheel::new();
    let handle = wheel.handle().clone();
    let id = handle.spawn("some name", async move {}).unwrap();
    handle.with_name(id, |_| {
        wheel.lock();//this should panic!
    });
}