
use juggle::dy::*;
use juggle::*;
use std::sync::{Mutex, Arc};
use std::future::Future;
use std::time::Duration;
use std::thread::*;
use std::task::*;
use std::pin::Pin;
use std::cell::Cell;


pub async fn panic_if(do_panic: &Cell<bool>) {
    loop {
        if do_panic.get() {
            unreachable!("Panic flag is set.");
        }
        yield_once!();
    }
}

#[derive(Clone)]
pub struct Signal(Arc<Mutex<(Option<Waker>, bool, usize)>>);

impl Future for Signal {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.lock().unwrap();
        guard.2 += 1; //inc poll count
        if guard.1 {
            guard.1 = false; //reset signal
            guard.0 = None;
            Poll::Ready(())
        } else {
            guard.0 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl Signal {
    pub fn new() -> Self { Self(Arc::new(Mutex::new((None, false, 0)))) }
    pub fn poll_count(&self) -> usize { self.0.lock().unwrap().2 }
    pub fn signal(&self, full: bool) {
        let mut guard = self.0.lock().unwrap();
        guard.1 = full;
        if let Some(waker) = guard.0.take() {
            waker.wake();
        }
    }
}

pub fn signal_after(dur: Duration) -> impl Future<Output=()> {
    let result = Signal::new();
    let ptr = result.clone();
    spawn(move || {
        sleep(dur);
        ptr.signal(true);
    });
    result
}