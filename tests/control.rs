mod common;
pub use common::*;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::task::{Context, Poll};
use std::time::Duration;
use rand::{Rng, SeedableRng};
use rand::prelude::StdRng;
use juggle::dy::*;
use juggle::*;
use juggle::utils::noop_waker;


#[test]
#[should_panic]
fn test_panic_task_really_panics() {
    smol::block_on(panic_if(&Cell::new(true)));
}

#[test]
fn test_suspend() {
    assert_eq!(SpawnParams::default(), SpawnParams::suspended(false));
    let panic1 = Cell::new(false);
    let panic2 = Cell::new(true);
    let wheel = Wheel::new();
    let id1 = wheel.handle().spawn(SpawnParams::default(), panic_if(&panic1)).unwrap();
    let id2 = wheel.handle().spawn(SpawnParams::suspended(true), panic_if(&panic2)).unwrap();
    let handle = wheel.handle().clone();
    let panic1 = &panic1;
    let panic2 = &panic2;
    wheel.handle().spawn(SpawnParams::named("Control"), async move {
        Yield::times(10).await;
        assert_eq!(handle.get_current_name().as_deref(), Some("Control"));
        //test resume/suspend
        assert!(handle.suspend(id1));
        assert_eq!(handle.get_state(id1), Some(State::Suspended));
        panic1.set(true);
        assert!(handle.resume(id2));
        assert_eq!(handle.get_state(id2), Some(State::Runnable));
        panic2.set(false);
        Yield::times(5).await;

        assert!(handle.resume(id1));
        panic1.set(false);
        Yield::times(5).await;

        //double suspend/resume
        assert!(handle.suspend(id1));
        assert_eq!(handle.get_state(id1), Some(State::Suspended));
        assert!(!handle.suspend(id1));
        assert_eq!(handle.get_state(id1), Some(State::Suspended));
        panic1.set(true);
        Yield::times(5).await;
        assert!(handle.resume(id1));
        assert_eq!(handle.get_state(id2), Some(State::Runnable));
        assert!(!handle.resume(id1));
        assert_eq!(handle.get_state(id2), Some(State::Runnable));
        panic1.set(false);

        let rand = &mut StdRng::seed_from_u64(1234);
        //spam resume/suspend on tasks
        for _ in 0..300 {
            for _ in 0..rand.gen_range(5, 150) {
                match rand.gen_range(0, 4) {
                    0 => {
                        handle.suspend(id1);
                        panic1.set(true);
                    }
                    1 => {
                        handle.suspend(id2);
                        panic2.set(true);
                    }
                    2 => {
                        handle.resume(id1);
                        panic1.set(false);
                    }
                    _ => {
                        handle.resume(id2);
                        panic2.set(false);
                    }
                }
            }
            if rand.gen_bool(0.3) {
                yield_once!();
            }
        }

        yield_once!();
        assert!(handle.cancel(id1));
        assert!(handle.cancel(id2));
    }).unwrap();

    smol::block_on(wheel).unwrap();
}

async fn self_suspend(handle: WheelHandle<'_>, after: usize) {
    let id = handle.current().unwrap();
    for _ in 0..after {
        yield_once!();
        assert_eq!(id, handle.current().unwrap());
    }
    handle.suspend(id);
}


#[test]
fn test_suspend_error() {
    let assert_flags = &vec![Cell::new(false); 3];

    let wheel = Wheel::new();

    wheel.handle().spawn(SpawnParams::suspended(true), async move {});//dummy
    wheel.handle().spawn(SpawnParams::suspended(true), async move {});//dummy
    wheel.handle().spawn(SpawnParams::default(), self_suspend(wheel.handle().clone(), 10));
    wheel.handle().spawn(SpawnParams::default(), self_suspend(wheel.handle().clone(), 20));
    wheel.handle().spawn(SpawnParams::default(), self_suspend(wheel.handle().clone(), 30));
    wheel.handle().spawn(SpawnParams::default(), signal_after(Duration::from_millis(50)));
    wheel.handle().spawn(SpawnParams::default(), signal_after(Duration::from_millis(75)));
    wheel.handle().spawn(SpawnParams::default(), signal_after(Duration::from_millis(100)));
    let w1 = wheel.handle().spawn(SpawnParams::default(), signal_after(Duration::from_millis(125))).unwrap();
    let w2 = wheel.handle().spawn(SpawnParams::default(), signal_after(Duration::from_millis(150))).unwrap();
    let h = wheel.handle().clone();
    wheel.handle().spawn(SpawnParams::default(), async move {
        //poll all tasks at least once
        yield_once!();
        assert_eq!(h.get_state(w1), Some(State::Waiting));
        assert_eq!(h.get_state(w2), Some(State::Waiting));
        Yield::times(5).await;
        let id = h.current().unwrap();
        assert!(h.cancel(id));
        assert_eq!(h.get_state(id), Some(State::Cancelled));
        assert!(!h.cancel(id));
        let hdl = h.clone();
        //check if task is unknown after some time, id cannot be taken until current task that
        //cancelled it yields so newly spawned task will have different id.
        let new_id = h.spawn(SpawnParams::default(), async move {
            yield_once!();
            assert_eq!(hdl.get_state(id), Some(State::Inactive));
            assert_flags[1].set(true);
        }).unwrap();
        assert_ne!(new_id, id);
        assert_flags[0].set(true);
        yield_once!();
        unreachable!();
    }).unwrap();
    assert_flags[2].set(true);
    // all tasks should eventually be suspended and error should be raised cause it's not possible
    // to change state of any task because wheel can be controlled ony inside this thread.
    smol::block_on(wheel).expect_err("Error was expected instead of success.");
    //assert all critical points were reached
    assert_flags.iter().for_each(|c| assert!(c.get()));
}

#[test]
fn test_signal() {
    let mut val = Box::pin(Signal::new());
    assert_eq!(val.poll_count(), 0);
    val.signal(true);
    let waker = noop_waker();
    let ctx = &mut Context::from_waker(&waker);
    assert_eq!(val.as_mut().poll(ctx), Poll::Ready(()));
    assert_eq!(val.poll_count(), 1);
    assert_eq!(Box::pin(Signal::new()).as_mut().poll(ctx), Poll::Pending);
}

#[test]
fn test_ready_task() {
    let signal = Signal::new();
    signal.signal(true); //make ready
    let wheel = Wheel::new();
    let ready = wheel.handle().spawn(SpawnParams::default(), signal.clone()).unwrap();
    let handle = wheel.handle().clone();
    assert_eq!(signal.poll_count(), 0);
    assert_eq!(handle.get_state(ready), Some(State::Runnable));
    wheel.handle().spawn(SpawnParams::default(), async move {
        yield_once!();
        assert_eq!(handle.get_state(ready), Some(State::Inactive));
        yield_once!();
    }).unwrap();
    smol::block_on(wheel).unwrap();
    assert_eq!(signal.poll_count(), 1);
}

#[test]
fn test_waiting() {
    let signal = Signal::new();
    let wheel = Wheel::new();
    let waiting = wheel.handle().spawn(SpawnParams::default(), signal.clone()).unwrap();
    let handle = wheel.handle().clone();
    assert_eq!(handle.get_state(waiting), Some(State::Runnable));//just created
    let ctrl = wheel.handle().spawn(SpawnParams::default(), async move {
        yield_once!();//wait for polling 'waiting' at least once.
        assert_eq!(handle.get_state(waiting), Some(State::Waiting));
        assert_eq!(signal.poll_count(), 1);
        handle.suspend(waiting);
        yield_once!();
        assert_eq!(handle.get_state(waiting), Some(State::Suspended));
        assert_eq!(signal.poll_count(), 1);
        Yield::times(10).await;
        assert_eq!(signal.poll_count(), 1);
        handle.resume(waiting);
        assert_eq!(handle.get_state(waiting), Some(State::Waiting));
        yield_once!();
        assert_eq!(signal.poll_count(), 1);
        assert_eq!(handle.get_state(waiting), Some(State::Waiting));
        signal.signal(false);
        assert_eq!(handle.get_state(waiting), Some(State::Runnable));
        Yield::times(2).await;
        assert_eq!(signal.poll_count(), 2);
        assert_eq!(handle.get_state(waiting), Some(State::Waiting));
        signal.signal(true);
        assert_eq!(handle.get_state(waiting), Some(State::Runnable));
        Yield::times(2).await;
        assert_eq!(signal.poll_count(), 3);
        assert_eq!(handle.get_state(waiting), Some(State::Inactive));//task completed
    }).unwrap();
    assert_ne!(waiting, ctrl);

    smol::block_on(wheel).unwrap();
}


#[test]
fn test_suspend_waiting() {
    let signal = Signal::new();
    let wheel = Wheel::new();
    let waiting = wheel.handle().spawn(SpawnParams::default(), signal.clone()).unwrap();
    let handle = wheel.handle().clone();
    wheel.handle().spawn(SpawnParams::default(), async move {
        yield_once!();
        assert_eq!(signal.poll_count(), 1);
        assert_eq!(handle.get_state(waiting), Some(State::Waiting));
        //after this task finishes 'waiting' should be only one task in scheduler.
        handle.suspend(waiting);
    }).unwrap();
    smol::block_on(wheel).expect_err("Expected SuspendError");
}

#[test]
fn test_dynamic_spawn() {
    async fn rec_bubble_sort<'a>(handle: WheelHandle<'a>, data: &'a RefCell<Vec<i32>>, len: usize) {
        let mut vec = data.borrow_mut();
        if len <= 1 { return; }
        for i in 0..len - 1 {
            yield_once!();
            if vec[i] > vec[i + 1] {
                vec.swap(i, i + 1);
            }
        }

        handle.spawn(SpawnParams::default(), rec_bubble_sort(handle.clone(), data, len - 1)).unwrap();
    }

    let mut vec = Vec::new();
    let mut rng = StdRng::seed_from_u64(12345);
    for _ in 0..1000 {
        vec.push(rng.gen());
    }
    let len = vec.len();
    let cell = RefCell::new(vec);
    let wheel = Wheel::new();

    wheel.handle().spawn(SpawnParams::default(), rec_bubble_sort(wheel.handle().clone(), &cell, len)).unwrap();

    smol::block_on(wheel).unwrap();

    let mut sorted = cell.borrow().to_vec();
    sorted.sort();
    assert_eq!(cell.into_inner(), sorted);
}