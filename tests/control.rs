use juggle::*;
use std::cell::Cell;
use rand::Rng;
use std::thread::{spawn, sleep};
use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Waker, Context, Poll};
use std::pin::Pin;

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
        assert_eq!(handle.get_state(id1),State::Suspended);
        panic1.set(true);
        assert!(handle.resume(id2));
        assert_eq!(handle.get_state(id2),State::Runnable);
        panic2.set(false);
        Yield::times(5).await;

        assert!(handle.resume(id1));
        panic1.set(false);
        Yield::times(5).await;

        //double suspend/resume
        assert!(handle.suspend(id1));
        assert_eq!(handle.get_state(id1),State::Suspended);
        assert!(!handle.suspend(id1));
        assert_eq!(handle.get_state(id1),State::Suspended);
        panic1.set(true);
        Yield::times(5).await;
        assert!(handle.resume(id1));
        assert_eq!(handle.get_state(id2),State::Runnable);
        assert!(!handle.resume(id1));
        assert_eq!(handle.get_state(id2),State::Runnable);
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
            if rand::thread_rng().gen_bool(0.3) {
                yield_once!();
            }
        }

        yield_once!();
        assert!(handle.cancel(id1));
        assert!(handle.cancel(id2));
    }).unwrap();

    smol::block_on(wheel).unwrap();
}

async fn self_suspend(handle: WheelHandle<'_>, after: usize){
    let id = handle.current().unwrap();
    for _ in 0..after {
        yield_once!();
        assert_eq!(id,handle.current().unwrap());
    }
    handle.suspend(id);
}

fn signal_after(dur: Duration)-> impl Future<Output=()>{
    struct Signal(Arc<Mutex<(Option<Waker>,bool)>>);
    impl Future for Signal{
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut guard = self.0.lock().unwrap();
            if guard.1 { Poll::Ready(()) }
            else{
                guard.0 = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    let ptr = Arc::new(Mutex::new((None,false)));
    let result = Signal(ptr.clone());
    spawn(move||{
        sleep(dur);
        let mut guard = ptr.lock().unwrap();
        guard.1 = true;
        if let Some(waker) = guard.0.take() {
            waker.wake();
        }
    });
    result
}


#[test]
fn test_suspend_error(){
    let assert_flags = &vec![Cell::new(false); 3];

    let wheel = Wheel::new();

    wheel.handle().spawn(SpawnParams::suspended(true),async move {});//dummy
    wheel.handle().spawn(SpawnParams::suspended(true),async move {});//dummy
    wheel.handle().spawn(SpawnParams::default(),self_suspend(wheel.handle().clone(),10));
    wheel.handle().spawn(SpawnParams::default(),self_suspend(wheel.handle().clone(),20));
    wheel.handle().spawn(SpawnParams::default(),self_suspend(wheel.handle().clone(),30));
    wheel.handle().spawn(SpawnParams::default(),signal_after(Duration::from_millis(50)));
    wheel.handle().spawn(SpawnParams::default(),signal_after(Duration::from_millis(75)));
    wheel.handle().spawn(SpawnParams::default(),signal_after(Duration::from_millis(100)));
    let w1 = wheel.handle().spawn(SpawnParams::default(),signal_after(Duration::from_millis(125))).unwrap();
    let w2 = wheel.handle().spawn(SpawnParams::default(),signal_after(Duration::from_millis(150))).unwrap();
    let h = wheel.handle().clone();
    wheel.handle().spawn(SpawnParams::default(),async move {
        //poll all tasks at least once
        yield_once!();
        assert_eq!(h.get_state(w1),State::Waiting);
        assert_eq!(h.get_state(w2),State::Waiting);
        Yield::times(5).await;
        let id = h.current().unwrap();
        assert!(h.cancel(id));
        assert_eq!(h.get_state(id),State::Cancelled);
        assert!(!h.cancel(id));
        let hdl = h.clone();
        //check if task is unknown after some time, id cannot be taken until current task that
        //cancelled it yields so newly spawned task will have different id.
        let new_id = h.spawn(SpawnParams::default(),async move{
            yield_once!();
            assert_eq!(hdl.get_state(id),State::Unknown);
            assert_flags[1].set(true);
        }).unwrap();
        assert_ne!(new_id,id);
        assert_flags[0].set(true);
        yield_once!();
        unreachable!();
    });
    assert_flags[2].set(true);
    // all tasks should eventually be suspended and error should be raised cause it's not possible
    // to change state of any task because wheel can be controlled ony inside this thread.
    smol::block_on(wheel).expect_err("Error was expected instead of success.");
    //assert all critical points were reached
    assert_flags.iter().for_each(|c|assert!(c.get()));
}