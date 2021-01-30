use core::future::Future;
use std::task::{Context, Poll, Waker};
use std::pin::Pin;
use core::sync::atomic::*;
use std::cell::{UnsafeCell, Cell};
use std::marker::PhantomData;

///Interrupt safe signalling.
pub struct IrqSignal{
    value: AtomicU8,
    borrow: AtomicBool,
    waker: UnsafeCell<Option<Waker>>,
}
pub struct SignalListen<'a>{
    signal: &'a IrqSignal,
    _phantom: PhantomData<Cell<usize>>, //not sync
}

const EXT_LOCK: u8 = 1;
const NOTIFIED: u8 = 2;
const IRQ_LOCK: u8 = 4;

impl IrqSignal{

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
            //not locked by irq and not notified
            let prev = self.value.fetch_or(IRQ_LOCK | NOTIFIED,Ordering::AcqRel);
            if prev & (IRQ_LOCK | EXT_LOCK | NOTIFIED) != 0 { return; }//abort, other irq or thread is accessing waker
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
            if prev & NOTIFIED != 0 {


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