use juggle::st::{StaticHandle, StaticParams, StaticWheelDef};
use juggle::*;
use core::time::Duration;
use rand::Rng;

async fn waiting_task(handle: StaticHandle) {
    println!("Wait Task [{}] enter", handle.get_current_name().unwrap_or(""));
    for i in 1..20 {
        let dur = Duration::from_millis(rand::thread_rng().gen_range(10, 20));
        smol::Timer::after(dur).await;
        println!("Wait Task [{}] point {}", handle.get_current_name().unwrap_or(""), i);
    }
    smol::Timer::after(Duration::from_millis(3)).await;
    println!("Wait Task [{}] exit", handle.get_current_name().unwrap_or(""));
}

async fn test_task(handle: StaticHandle) {
    println!("Task [{}] enter", handle.get_current_name().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 1", handle.get_current_name().unwrap_or(""));
    if handle.get_current_name() == Some("T1") {
        assert!(handle.resume(handle.get_by_name("WT11").unwrap()));
        assert!(handle.resume(handle.get_by_name("WT12").unwrap()));
        assert!(handle.resume(handle.get_by_name("WT13").unwrap()));
    }
    yield_once!();
    println!("Task [{}] point 2", handle.get_current_name().unwrap_or(""));
    yield_once!();
    println!("Task [{}] point 3", handle.get_current_name().unwrap_or(""));
    yield_once!();
    println!("Task [{}] exit", handle.get_current_name().unwrap_or(""));
}

#[test]
pub fn test_round_robin() {
    static WHEEL: StaticWheelDef = static_config!{
        (handle) StaticParams::named("T1") => test_task(handle),
        (handle) StaticParams::named("WT11").suspend(true) => waiting_task(handle),
        (handle) StaticParams::named("T2") => test_task(handle),
        (handle) StaticParams::named("WT12").suspend(true) => waiting_task(handle),
        (handle) StaticParams::named("T3") => test_task(handle),
        (handle) StaticParams::named("WT13").suspend(true) => waiting_task(handle),
        (handle) StaticParams::named("T4") => test_task(handle)
    };
    let wheel = WHEEL.lock();
    smol::block_on(wheel).unwrap();
    println!("Finished round robin.");
}
