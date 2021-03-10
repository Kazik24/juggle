use core::future::Future;
use core::task::{Context, Poll, Waker};
use core::pin::Pin;
use core::sync::atomic::*;
use core::cell::{UnsafeCell, Cell};
use core::marker::PhantomData;
use smallvec::Array;

///Interrupt safe signalling.
pub struct IrqSignal{
    value: AtomicU8,
    borrow: AtomicBool,
    waker: UnsafeCell<Option<Waker>>,
}
unsafe impl Send for IrqSignal {}
unsafe impl Sync for IrqSignal {}

pub struct SignalListen<'a>{
    signal: &'a IrqSignal,
    _phantom: PhantomData<Cell<usize>>, //not sync
}

const EXT_LOCK: u8 = 1;
const NOTIFIED: u8 = 2;
const IRQ_LOCK: u8 = 4;

impl IrqSignal{

    pub const fn new()->Self{
        Self{
            value: AtomicU8::new(0),
            borrow: AtomicBool::new(false),
            waker: UnsafeCell::new(None),
        }
    }

    ///Interrupt safe notification
    pub fn notify(&self){
        loop{
            let prev = self.value.load(Ordering::Acquire);
            if prev & NOTIFIED != 0 { return; }
            if prev & EXT_LOCK != 0 { //sth was changing waker
                let next = prev | NOTIFIED;
                if self.value.compare_and_swap(prev,next,Ordering::AcqRel) == prev { return; }
                continue;
            }
            //not locked by irq and not notified, try acquire irq lock
            loop{
                let prev = self.value.load(Ordering::Acquire);
                if prev & (IRQ_LOCK | EXT_LOCK) != 0 {
                    if prev & NOTIFIED == 0 {
                        //if not notified try to sneak in NOTIFIED flag
                        self.value.fetch_or(NOTIFIED,Ordering::AcqRel);
                    }
                    //abort, other irq or thread is accessing waker
                    return;
                }
                let next = prev | (IRQ_LOCK | NOTIFIED);
                if self.value.compare_and_swap(prev,next,Ordering::AcqRel) == prev { break; }
            }
            //exclusive lock on waker
            unsafe{
                //wake by reference, do not drop waker!
                match &*self.waker.get() {
                    Some(ref w) => w.wake_by_ref(),
                    None => {}
                }
            }
            //change IRQ_LOCK | NOTIFIED to NOTIFIED
            self.value.store(NOTIFIED,Ordering::Release);
            return;
        }
    }

    pub fn try_listen(&self)->Option<SignalListen>{
        if !self.borrow.compare_and_swap(false,true,Ordering::AcqRel) {
            Some(SignalListen{signal: self,_phantom: PhantomData})
        }else{
            None
        }
    }
    pub fn listen(&self)->SignalListen{
        self.try_listen().expect("Listener already registered.")
    }
}

impl<'a> Future for SignalListen<'a>{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let signal = self.signal;

        //try lock signal
        loop{
            let prev = signal.value.load(Ordering::Acquire);
            //cannot hold external lock now cause only one thread can poll future
            assert!(prev & EXT_LOCK == 0);
            if prev & NOTIFIED != 0 {
                //signal was notified, attempt to remove exploited waker and drop it.
                if prev & IRQ_LOCK != 0 {
                    spin_loop_hint();
                    continue; //locked by irq, wait for it to finish so loop once again
                }
                //try set external lock on value
                if signal.value.compare_and_swap(prev,NOTIFIED | EXT_LOCK,Ordering::AcqRel) != prev {
                    spin_loop_hint();
                    continue; // failed to set external lock, try again.
                }
                // now state is NOTIFIED | EXT_LOCK
                // SAFETY: so we have exclusive lock on waker, try to drop it's value.
                let waker_to_drop = unsafe{ (*signal.waker.get()).take() };
                // keep waker for now, and setup proper state before dropping it.
                signal.value.store(0,Ordering::Release);
                drop(waker_to_drop); //now drop it
                return Poll::Ready(());
            }else{
                //attempt to register waker
                let prev = signal.value.fetch_or(EXT_LOCK,Ordering::AcqRel);


            }
        }


        todo!()
    }
}

impl Drop for SignalListen<'_>{
    fn drop(&mut self) {
        self.signal.borrow.store(false,Ordering::Release);
    }
}


#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::thread::spawn;

    fn test_one_thread(){
        static SIGNAL: IrqSignal = IrqSignal::new();
        let handle = spawn(move||{
            for _ in 0..10 {
                SIGNAL.notify();
            }
        });
        let future = SIGNAL.listen();


        handle.join().unwrap();
    }
}