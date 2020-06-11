use juggle::*;
use std::cell::Cell;
use rand::Rng;
use std::thread::spawn;

async fn panic_if(do_panic: &Cell<bool>){
    loop{
        if do_panic.get() {
            unreachable!("Panic flag is set.");
        }
        yield_once!();
    }
}

#[test]
#[should_panic]
fn test_panic_task_really_panics(){
    smol::block_on(panic_if(&Cell::new(true)));
}

#[test]
fn test_suspend(){
    assert_eq!(SpawnParams::default(),SpawnParams::suspended(false));
    let panic1 = Cell::new(false);
    let panic2 = Cell::new(true);
    let wheel = Wheel::new();
    let id1 = wheel.handle().spawn(SpawnParams::default(),panic_if(&panic1)).unwrap();
    let id2 = wheel.handle().spawn(SpawnParams::suspended(true),panic_if(&panic2)).unwrap();
    let handle = wheel.handle().clone();
    let panic1 = &panic1;
    let panic2 = &panic2;
    wheel.handle().spawn(SpawnParams::named("Control"),async move{
        Yield::times(10).await;
        assert_eq!(handle.get_current_name().as_deref(),Some("Control"));
        //test resume/suspend
        assert!(handle.suspend(id1));
        panic1.set(true);
        assert!(handle.resume(id2));
        panic2.set(false);
        Yield::times(5).await;

        assert!(handle.resume(id1));
        panic1.set(false);
        Yield::times(5).await;

        //double suspend/resume
        assert!(handle.suspend(id1));
        assert!(!handle.suspend(id1));
        panic1.set(true);
        Yield::times(5).await;
        assert!(handle.resume(id1));
        assert!(!handle.resume(id1));
        panic1.set(false);

        //spam resume/suspend on tasks
        for _ in 0..300 {
            for _ in 0..rand::thread_rng().gen_range(5,150) {
                match rand::thread_rng().gen_range(0,4) {
                    0 => {handle.suspend(id1); panic1.set(true);}
                    1 => {handle.suspend(id2); panic2.set(true);}
                    2 => {handle.resume(id1); panic1.set(false);}
                    _ => {handle.resume(id2); panic2.set(false);}
                }
            }
            if rand::thread_rng().gen_bool(0.1) {
                yield_once!();
            }
        }

        yield_once!();
        assert!(handle.cancel(id1));
        assert!(handle.cancel(id2));
    }).unwrap();

    smol::block_on(wheel).unwrap();
}