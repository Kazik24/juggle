use alloc::sync::Arc;
use core::sync::atomic::*;
use std::task::Waker;
use std::cell::UnsafeCell;
use std::mem::{ManuallyDrop, replace, forget};
use std::ops::{Deref, DerefMut};
use std::ptr::read;

pub struct SignalSend(Arc<Signal>);
pub struct SignalRecv(Arc<Signal>);


struct AtomicSwap<T>{
    mark: AtomicUsize,
    cell: UnsafeCell<ManuallyDrop<T>>,
}
unsafe impl<T: Send + Sync> Send for AtomicSwap<T> {}
unsafe impl<T: Send + Sync> Sync for AtomicSwap<T> {}

impl<T> AtomicSwap<T>{

    pub fn new(value: T)->Self{
        Self{
            mark: AtomicUsize::new(0),
            cell: UnsafeCell::new(ManuallyDrop::new(value)),
        }
    }

    fn try_swap_internal(&self,value: ManuallyDrop<T>)->Result<ManuallyDrop<T>,ManuallyDrop<T>>{
        let prev = self.mark.fetch_add(1,Ordering::Acquire);
        if prev == 0 { //we know for sure we are only thread writing to this location
            //swap values
            unsafe{
                let first = self.cell.get().read_volatile();
                self.cell.get().write_volatile(value);
                self.mark.fetch_sub(1,Ordering::Release);
                Ok(first)
            }
        }else { //we are some other thread waiting for this location
            self.mark.fetch_sub(1, Ordering::Release);//restore mark count
            Err(value)
        }
    }
    pub fn try_apply(&self,func: impl FnOnce(&mut T))->Result<(),()>{
        let prev = self.mark.fetch_add(1,Ordering::Acquire);
        if prev == 0 { //we know for sure we are only thread writing to this location
            //swap values
            unsafe{
                let mut first = self.cell.get().read_volatile();
                func(&mut first.deref_mut());
                self.cell.get().write_volatile(first);
                self.mark.fetch_sub(1,Ordering::Release);
                Ok(())
            }
        }else { //we are some other thread waiting for this location
            self.mark.fetch_sub(1, Ordering::Release);//restore mark count
            Err(())
        }
    }

    pub fn try_swap(&self,value: T)->Result<T,T>{
        match self.try_swap_internal(ManuallyDrop::new(value)) {
            Ok(val) => Ok(ManuallyDrop::into_inner(val)),
            Err(val) => Err(ManuallyDrop::into_inner(val)),
        }
    }

    pub fn swap(&self,value: T)->T{
        let mut value = ManuallyDrop::new(value);
        loop{
            unsafe{
                match self.try_swap_internal(read(&value as *const _)) {
                    Ok(val) => return ManuallyDrop::into_inner(val),
                    Err(val) =>{
                        value = val;
                        spin_loop_hint();
                    }
                }
            }
        }
    }
    pub fn apply(&self,mut func: impl FnMut(&mut T)){
        loop{
            match self.try_apply(|r|func(r)) {
                Ok(_) => break,
                Err(_) => spin_loop_hint(),
            }
        }
    }
    pub fn into_inner(mut self)->T{
        unsafe{
            let data = read(self.cell.get());
            forget(self);//don't run destructor
            ManuallyDrop::into_inner(data)
        }
    }
}

impl<T> Drop for AtomicSwap<T>{
    fn drop(&mut self) {
        unsafe{
            ManuallyDrop::drop(&mut *self.cell.get());
        }
    }
}



struct Signal{
    mod_count: AtomicUsize,
    send_count: AtomicUsize,
    waker: AtomicSwap<Option<Waker>>,


}
impl Signal{
    pub fn new()->Self{
        Self{
            mod_count: AtomicUsize::new(0),
            send_count: AtomicUsize::new(0),
            waker: AtomicSwap::new(None),
        }
    }
    pub fn send_one(&self)->bool{
        let prev = self.send_count.fetch_add(1,Ordering::AcqRel);
        match self.waker.try_swap(None) {
            Ok(Some(waker)) =>{
                waker.wake();
                return true;
            },
            _ => {}
        }
        false
    }
    pub fn receive(&self,waker: &Waker)->usize{
        let prev = self.send_count.swap(0,Ordering::AcqRel);
        if prev == 0 {
            let mut waker = waker.clone();
            let waker = self.waker.swap(Some(waker));
            let next = self.send_count.swap(0,Ordering::AcqRel);
            if next != 0 {

            }
        }
        prev
    }
}
unsafe impl Send for Signal{}
unsafe impl Sync for Signal{}


#[cfg(test)]
mod test{
    use std::sync::Arc;
    use super::*;
    use std::thread::{JoinHandle, spawn};
    use std::future::Future;
    use std::task::{Context, Poll};
    use std::pin::Pin;
    use std::io::{stdout, Write};
    use std::collections::HashSet;


    struct Wrapped(Arc<Signal>);
    impl Future for Wrapped{
        type Output = usize;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let res = self.0.receive(cx.waker());
            if res == 0 {Poll::Pending}
            else { Poll::Ready(res) }
        }
    }

    fn spawn_thread(signal: Arc<Signal>,times: usize)->JoinHandle<()>{
        spawn(move ||{
            for _ in 0..times {
                signal.send_one();
            }
        })
    }

    #[test]
    #[ignore]
    fn test_statistical(){
        let mut sout = stdout();
        writeln!(sout,"Started");
        let per_thread = 100000;
        let threads = 4;

        let signal = Arc::new(Signal::new());

        let all = (0..threads).map(|_|spawn_thread(signal.clone(),per_thread)).collect::<Vec<_>>();
        smol::block_on(async move {
            let mut sum = 0;
            loop{
                writeln!(sout,"asdasd");
                sum += Wrapped(signal.clone()).await;
                if sum >= per_thread*threads {
                    break;
                }
            }
        });

        for h in all { h.join(); }
    }



    #[test]
    fn test_swap_single_thread(){
        let swap = AtomicSwap::new(1);
        assert_eq!(swap.try_swap(2),Ok(1));
        assert_eq!(swap.swap(3),2);
        assert_eq!(swap.swap(12345),3);
        swap.try_apply(|val|{
            assert_eq!(*val,12345);
            *val = 10;
        });
        assert_eq!(swap.swap(0),10);
    }


    #[test]
    fn test_atomic_swap(){
        let threads = 10;
        let per_thread = 10000;



        let swap = Arc::new(AtomicSwap::new(None));




        let thr = (0..threads).map(|t|{
            let mut v = Vec::new();
            for i in 0..per_thread{
                v.push(Some(t*per_thread + i + 1));
            }
            (v,swap.clone())
        }).map(|(vec,swap)|spawn(move ||{
            let mut res = Vec::new();
            for val in vec {
                res.push(swap.swap(val));
            }
            res
        })).collect::<Vec<_>>();
        let mut data = thr.into_iter().map(|j|j.join().unwrap()).flatten().collect::<HashSet<_>>();
        data.insert(swap.swap(None));
        assert!(data.contains(&None));
        let res = (1..(per_thread*threads + 1)).filter(|v|!data.contains(&Some(*v))).collect::<Vec<_>>();
        assert!(res.is_empty(),"Results not empty {:#?}",&res);
    }
}