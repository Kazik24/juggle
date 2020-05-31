use core::future::Future;
use core::task::{Context, Poll};
use core::pin::Pin;
use alloc::rc::Rc;
use std::time::{Duration, Instant};
use core::cell::RefCell;
use core::ops::{Deref, Mul};
use std::marker::PhantomData;
use crate::timing::{StdTiming, Timing, TimePoint};
use crate::chunk_slab::ChunkSlab;
use std::cmp::{max, min};
use crate::TimingGroup;


struct GenericLoadBalance<F: Future,I: Timing>{
    index: usize,
    group: Rc<RefCell<TimingGroup<I>>>,
    future: F,
}


impl<F: Future,I: Timing> GenericLoadBalance<F,I>{
    pub fn new(prop: u32,future: F)->Self{
        let mut group = TimingGroup::new();
        let key = group.add(prop);
        Self{
            index: key,
            group: Rc::new(RefCell::new(group)),
            future
        }
    }
    pub fn add<G>(&mut self,prop: u32,future: G)->GenericLoadBalance<G,I> where G: Future{
        let index = self.group.borrow_mut().add(prop);
        GenericLoadBalance{
            index,
            group: self.group.clone(), //clone rc
            future,
        }
    }

}


impl<F: Future,I: Timing> Drop for GenericLoadBalance<F,I>{
    fn drop(&mut self) {
        self.group.borrow_mut().remove(self.index);
    }
}

impl<F: Future,I: Timing> Future for GenericLoadBalance<F,I>{
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        if !self.group.borrow().can_execute(self.index) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let start = I::start();
        let res = unsafe{ Pin::new_unchecked(&mut self.as_mut().get_unchecked_mut().future).poll(cx) };
        self.group.borrow_mut().update_duration(self.index,start.duration_from());

        return res;
    }
}



pub struct LoadBalance<F: Future>{
    inner: GenericLoadBalance<F,StdTiming>,
}
impl<F: Future> LoadBalance<F>{
    pub fn with(prop: u32,future: F)->Self{
        Self{inner: GenericLoadBalance::new(prop,future)}
    }
    pub fn add<G>(&mut self,prop: u32,future: G)->LoadBalance<G> where G: Future{
        LoadBalance{inner:self.inner.add(prop,future)}
    }
}
impl<F: Future> Future for LoadBalance<F>{
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        unsafe{Pin::new_unchecked(&mut self.get_unchecked_mut().inner)}.poll(cx)
    }
}

