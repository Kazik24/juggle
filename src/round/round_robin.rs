use std::collections::{VecDeque, LinkedList};
use core::future::Future;
use core::task::{Context, Poll, Waker};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::ptr::NonNull;
use super::dyn_future::DynamicFuture;
use crate::utils::AtomicWakerRegistry;
use crate::round::dyn_future::TaskName;

//round robin scheduling
pub struct Wheel{ //not clone so that rc has strong count of 1
    ptr: Rc<UnsafeCell<InnerRoundRobin>>,
}

#[derive(Clone)]
pub struct WheelHandle{
    ptr: Weak<UnsafeCell<InnerRoundRobin>>,
}

pub struct LockedWheel{
    inner: InnerRoundRobin,
}
struct InnerRoundRobin{
    runnable0: VecDeque<DynamicFuture>,
    runnable1: VecDeque<DynamicFuture>,
    deferred: LinkedList<DynamicFuture>,
    last_waker: Arc<AtomicWakerRegistry>,
    which_buffer: bool,
    current_ptr: Option<NonNull<DynamicFuture>>, //stack allocated!!!
}

// pub struct SpawnBuilder<'a>{
//     handle: &'a mut WheelHandle,
// }
// pub struct PropertiesBuilder<'a>{
//     handle: &'a mut WheelHandle,
//     dynamic: DynamicFuture,
// }

impl WheelHandle{
    pub fn is_valid(&self)->bool{ self.ptr.strong_count() != 0 }
    pub fn spawn<F: Future<Output=()> + 'static>(&self,future: F)->bool{
        self.spawn_dyn(String::new(),Box::pin(future))
    }
    pub fn spawn_named<F: Future<Output=()> + 'static>(&self,name: impl Into<String>,future: F)->bool{
        self.spawn_dyn(name.into(),Box::pin(future))
    }
    pub fn spawn_dyn(&self,name: String,future: Pin<Box<dyn Future<Output=()> + 'static>>)->bool{
        let rc = match self.ptr.upgrade() {
            Some(v) => v,
            None => return false,
        };
        let this = Self::unchecked_mut(&rc);
        let mut future = DynamicFuture::new_allocated(future,this.last_waker.clone(),false);
        future.set_name(TaskName::Dynamic(name.into_boxed_str()));
        if this.which_buffer { //if now 1 is executed then add to 0 and vice versa.
            this.runnable0.push_back(future);
        } else {
            this.runnable1.push_back(future);
        }
        true
    }
    fn unchecked_mut(rc: &Rc<UnsafeCell<InnerRoundRobin>>)->&mut InnerRoundRobin{
        unsafe{ &mut *rc.get() }
    }

    pub fn print_tasks(&self,title: impl Into<String>){
        let rc = match self.ptr.upgrade() {
            Some(v) => v,
            None => {
                println!("*** Round robin tasks [{}]: INVALID HANDLE",title.into());
                return;
            }
        };
        let this = Self::unchecked_mut(&rc);
        println!("*** Round robin tasks [{}]:",title.into());
        let mut r0: Vec<_> = this.runnable0.iter().map(|t|t.get_name()).collect();
        let mut r1: Vec<_> = this.runnable1.iter().map(|t|t.get_name()).collect();
        println!("Runnable buffers:");
        if this.which_buffer {
            std::mem::swap(&mut r0,&mut r1);
        }
        println!(" main: {:?}",&r0);
        println!(" back: {:?}",&r1);
        println!("Deferred:");
        println!(" {:?}",this.deferred.iter().map(|t|t.get_name()).collect::<Vec<_>>());
        println!("Has waker: {}",!this.last_waker.is_empty());
    }

    //cancel task by id (make id some type, if possible reference this handle in it)
    // pub fn cancel(&self,id: u32){
    //
    // }
}


impl Wheel{
    pub fn new()->Self{
        let ptr = Rc::new(UnsafeCell::new(InnerRoundRobin{
            runnable0: VecDeque::new(),
            runnable1: VecDeque::new(),
            deferred: LinkedList::new(),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            which_buffer: false,
            current_ptr: None,
        }));
        Self{ptr}
    }
    pub fn handle(&self)->WheelHandle{ WheelHandle{ ptr: Rc::downgrade(&self.ptr) } }
    fn inner_mut(&self)->&mut InnerRoundRobin{
        unsafe{ &mut *self.ptr.get() }
    }
    //causes all handles to become invalid
    pub fn lock(self)->LockedWheel{
        // no panic cause rc has always strong count of 1 (it can have strong count > 1 during calls
        // on handle, but if these calls return then it will be back to 1)
        let inner = Rc::try_unwrap(self.ptr).ok().unwrap().into_inner();
        LockedWheel{inner}
    }

}

impl InnerRoundRobin {
    fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop{
            self.last_waker.clear();//drop previous waker if any
            if self.which_buffer {
                if Self::beat_once(&mut self.runnable1,&mut self.runnable0,
                                   &mut self.deferred,cx.waker(),&self.last_waker,&mut self.current_ptr) {
                    return Poll::Pending;
                }
            }else{
                if Self::beat_once(&mut self.runnable0,&mut self.runnable1,
                                   &mut self.deferred,cx.waker(),&self.last_waker,&mut self.current_ptr) {
                    return Poll::Pending;
                }
            }
            self.which_buffer = !self.which_buffer;
            if self.runnable0.is_empty() && self.runnable1.is_empty() && self.deferred.is_empty() {break;}
        }
        Poll::Ready(()) //all tasks executed to finish
    }

    fn rotate_once(from: &mut VecDeque<DynamicFuture>,to: &mut VecDeque<DynamicFuture>,
                   deferred: &mut LinkedList<DynamicFuture>,current_ptr: &mut Option<NonNull<DynamicFuture>>){
        struct Guard<'a>(&'a mut Option<NonNull<DynamicFuture>>);
        impl<'a> Drop for Guard<'a>{ fn drop(&mut self) { *self.0 = None; } }

        while let Some(mut run) = from.pop_front() {

            *current_ptr = NonNull::new(&mut run as *mut _);
            let guard = Guard(current_ptr);
            // be careful with interior mutability types here cause poll_local can invoke any method
            // on handle, therefore 'from' queue shouldn't be edited by handles (other structures
            // are pretty much ok, actually even 'from' queue is ok with this cause it is not
            // borrowed by iterators or other such things but it might disturb task processing)
            let result = run.poll_local().is_pending();
            drop(guard);


            if result { //reconsider enqueuing this future again
                if run.is_runnable() { //if immediately became runnable then enqueue it
                    to.push_back(run);
                } else { //put it on deferred queue
                    deferred.push_back(run);
                }
            }
        }
    }

    fn drain_runnable(from: &mut LinkedList<DynamicFuture>,to: &mut VecDeque<DynamicFuture>)->bool{
        let mut mark = false;
        for _ in 0..from.len() {
            if let Some(v) = from.pop_front(){
                if v.is_runnable() {
                    to.push_back(v);
                    mark = true;
                }else{
                    from.push_back(v); //return to list
                }
            }else {break;}
        }
        mark
    }

    fn beat_once(from: &mut VecDeque<DynamicFuture>,to: &mut VecDeque<DynamicFuture>,
                 deferred: &mut LinkedList<DynamicFuture>,waker: &Waker,
                 last_waker: &AtomicWakerRegistry,current_ptr: &mut Option<NonNull<DynamicFuture>>)->bool{

        if !deferred.is_empty() && !Self::drain_runnable(deferred,from){
            if from.is_empty() { //if has no work to do
                //no runnable task found, register waker
                last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if !Self::drain_runnable(deferred,from) {
                    //waiting begins
                    return true;//true means that future should wait for waker
                }
                //if any was woken then try to deregister waker, then make one rotation
                last_waker.clear();
            }
        }
        Self::rotate_once(from,to,deferred,current_ptr);
        false //can start new iteration
    }
}

impl Future for Wheel{
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner_mut().poll_internal(cx)
    }
}

impl Future for LockedWheel{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_mut().inner.poll_internal(cx)
    }
}