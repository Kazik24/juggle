

mod common;
pub use common::*;
use juggle::dy::*;
use juggle::*;
use rand::prelude::StdRng;
use rand::*;
use std::rc::Rc;
use rand::seq::SliceRandom;
use std::future::Future;


fn generate_arbitrary_waits(handle: &WheelHandle<'_>,batch: usize,max_wait: usize,max_rep: usize,count: usize,rand: &mut StdRng){
    let batches = (0..count)
        .map(|_|(0..batch).map(|_|Signal::new()).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let batches = Rc::new(batches);
    let mut tasks = Vec::new();
    for (i,batch) in batches.iter().enumerate() {
        for (j,_) in batch.iter().enumerate() {
            let gen_before = rand.gen_range(0,max_wait);
            let gen_wait = rand.gen_range(0,max_wait);
            let gen_after = rand.gen_range(0,max_wait);
            let gen_repeats = rand.gen_range(0,max_rep);
            let batches = batches.clone();
            tasks.push(async move{
                Yield::times(gen_before).await;
                if i != 0 {
                    let sig = batches[i-1][j].clone();
                    sig.await; //wait for previous batch assignee
                }

                for _ in 0..gen_repeats{
                    Yield::times(gen_wait).await;
                    batches[i][j].signal(false); //dummy signal
                }
                batches[i][j].signal(true); //signal for next batch

                Yield::times(gen_after).await;
            });
        }
    }
    tasks.shuffle(rand);
    for t in tasks {
        handle.spawn_default(t).unwrap();
    }
}


#[test]
fn test_arbitrary_waits(){

    let rand = &mut StdRng::seed_from_u64(69420); //nice
    for _ in 0..200 {
        let wheel = Wheel::new();
        let batch = rand.gen_range(1,10);
        let wait = rand.gen_range(1,20);
        let reps = rand.gen_range(1,5);
        let count = rand.gen_range(1,100);
        generate_arbitrary_waits(wheel.handle(),batch,wait,reps,count,rand);
        smol::block_on(wheel).unwrap();
    }

}

fn timeout_wheel<'a,F,T>(loops: usize,func: F) where F: FnOnce(WheelHandle<'a>)->T + 'a, T: Future<Output=()> + 'a{
    let wheel = Wheel::new();
    let handle = wheel.handle().clone();
    wheel.handle().spawn_default(async move {
        Yield::times(loops).await;
        yield_once!();
        if handle.registered_count() > 1 { //this task is not alone
            unreachable!("Test timed out after {} yields",loops); //bomb
        }
    }).unwrap();
    let handle = wheel.handle().clone();
    wheel.handle().spawn_default(func(handle)).unwrap();
    smol::block_on(wheel).unwrap();
}

#[test]
#[should_panic]
fn test_timeout_works(){
    timeout_wheel(100,|_| async move {
        Yield::times(101).await;
    });
}

#[test]
fn test_cancel_waiting(){
    timeout_wheel(100,|handle| async move {
        let id = handle.spawn_default(Signal::new()).unwrap();
        Yield::times(2).await;
        assert_eq!(handle.get_state(id),State::Waiting);
        assert!(handle.cancel(id));
        assert_eq!(handle.get_state(id),State::Cancelled);
        yield_once!();
        assert_eq!(handle.get_state(id),State::Unknown);
    });
}

#[test]
fn test_cancel_suspended(){
    timeout_wheel(100,|handle| async move {
        let id = handle.spawn(SpawnParams::suspended(true),async{}).unwrap();
        assert_eq!(handle.get_state(id),State::Suspended);
        assert!(handle.cancel(id));
        assert_eq!(handle.get_state(id),State::Cancelled);
        yield_once!();
        assert_eq!(handle.get_state(id),State::Unknown);
    });
}

#[test]
fn test_cancel_suspended_delay(){
    timeout_wheel(100,|handle| async move {
        let id = handle.spawn(SpawnParams::suspended(true),async{}).unwrap();
        assert_eq!(handle.get_state(id),State::Suspended);
        Yield::times(10).await;
        assert_eq!(handle.get_state(id),State::Suspended);
        assert!(handle.cancel(id));
        assert_eq!(handle.get_state(id),State::Cancelled);
        yield_once!();
        assert_eq!(handle.get_state(id),State::Unknown);
    });
}

#[test]
fn test_suspend_while_wait(){
    timeout_wheel(100,|handle| async move {
        let signal = Signal::new();
        let id = handle.spawn_default(signal.clone()).unwrap();
        yield_once!();
        assert_eq!(handle.get_state(id),State::Waiting);
        assert!(handle.suspend(id));
        assert_eq!(handle.get_state(id),State::Suspended);
        Yield::times(10).await;
        assert_eq!(handle.get_state(id),State::Suspended);
        assert!(handle.resume(id));
        assert_eq!(handle.get_state(id),State::Waiting);
        signal.signal(true);
        assert_eq!(handle.get_state(id),State::Runnable);
        Yield::times(2).await;
        assert_eq!(handle.get_state(id),State::Unknown);
    });
}