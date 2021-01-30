mod common;
pub use common::*;
use juggle::*;
use juggle::utils::*;
use std::future::Future;
use std::pin::Pin;
use std::task::*;
use std::sync::atomic::*;
use std::sync::Arc;
use std::cell::Cell;



struct UnderTest<'a>{
    wheel: Pin<Box<Wheel<'a>>>,
    count: Arc<AtomicUsize>,
    waker: Waker,
}
impl<'a> UnderTest<'a>{
    pub fn new()->Self{
        let count = Arc::new(AtomicUsize::new(0));
        Self{
            wheel: Box::pin(Wheel::new()),
            count: count.clone(),
            waker: to_waker(Arc::new(move||{count.fetch_add(1,Ordering::SeqCst);})),
        }
    }
    pub fn poll_once(&mut self)->Poll<Result<(),SuspendError>>{
        self.wheel.as_mut().poll(&mut Context::from_waker(&self.waker))
    }
    pub fn wake_count(&self)->usize{ self.count.load(Ordering::SeqCst) }

}

async fn count_down(cd: &Cell<u32>){
    while cd.get() != 0 {
        cd.set(cd.get() - 1);
        yield_once!();
    }
}

#[test]
fn test_assert_state_simple(){
    let cd1 = &Cell::new(10);
    let cd2 = &Cell::new(20);
    let cd3 = &Cell::new(5);
    let mut test = UnderTest::new();
    assert_eq!(test.poll_once(),Poll::Ready(Ok(())));
    test.wheel.handle().spawn_default(count_down(cd1)).unwrap();
    test.wheel.handle().spawn_default(count_down(cd2)).unwrap();
    test.wheel.handle().spawn_default(count_down(cd3)).unwrap();

    assert_eq!(cd1.get(),10);
    assert_eq!(cd2.get(),20);
    assert_eq!(cd3.get(),5);
    assert_eq!(test.poll_once(),Poll::Ready(Ok(())));
    assert_eq!(cd1.get(),0);
    assert_eq!(cd2.get(),0);
    assert_eq!(cd3.get(),0);
}
#[test]
fn test_assert_suspend_error(){
    let cd1 = &Cell::new(30);
    let mut test = UnderTest::new();
    assert_eq!(test.poll_once(),Poll::Ready(Ok(())));
    test.wheel.handle().spawn_default(count_down(cd1)).unwrap();
    test.wheel.handle().spawn(SpawnParams::suspended(true),async{}).unwrap();

    assert_eq!(cd1.get(),30);
    assert_eq!(test.poll_once(),Poll::Ready(Err(SuspendError)));
    assert_eq!(cd1.get(),0);
}

#[test]
fn test_starving_bug(){
    let reached = Cell::new(false);
    let mut test = UnderTest::new();
    let handle = test.wheel.handle().clone();
    let signal = Signal::new();
    handle.spawn_default(signal.clone()).unwrap();
    assert_eq!(test.wake_count(),0);
    assert!(test.poll_once().is_pending());
    assert_eq!(test.wake_count(),0);
    assert!(test.poll_once().is_pending());
    assert_eq!(test.wake_count(),0);

    //main test case
    handle.spawn_default(async{
        reached.set(true);//set flag when this task is first polled
    }).unwrap();
    assert!(!reached.get());
    //one task is waiting but other was just spawned and should be polled
    assert!(test.poll_once().is_pending());
    assert!(reached.get(),"Task was not polled");
    signal.signal(true);
    assert_eq!(test.wake_count(),1);
    assert_eq!(test.poll_once(),Poll::Ready(Ok(())));
}

#[test]
fn test_self_suspend_bug(){
    let mut test = UnderTest::new();
    let handle = test.wheel.handle().clone();
    let id = test.wheel.handle().spawn_default(async move {
        let curr = handle.current().unwrap();
        handle.suspend(curr);
        handle.resume(curr);
        println!("Spammed");
        yield_once!();
        handle.cancel(curr);
        println!("Cancelled");
        yield_once!();
        unreachable!();
    }).unwrap();
    test.wheel.handle().spawn_default(Yield::times(100)).unwrap();
    assert!(test.poll_once().is_ready());
}

#[test]
fn test_self_suspend_spam_bug(){
    let mut test = UnderTest::new();
    let handle = test.wheel.handle().clone();
    let id = test.wheel.handle().spawn_default(async move {
        let curr = handle.current().unwrap();
        for _ in 0..10 {
            handle.suspend(curr);
            handle.resume(curr);
        }
        yield_once!();
        handle.cancel(curr);
        yield_once!();
        unreachable!();
    }).unwrap();
    test.wheel.handle().spawn_default(Yield::times(100)).unwrap();
    assert!(test.poll_once().is_ready());
}