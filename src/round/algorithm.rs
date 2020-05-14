use crate::round::dyn_future::DynamicFuture;
use std::collections::{VecDeque, HashSet, HashMap};
use std::sync::Arc;
use crate::utils::AtomicWakerRegistry;
use core::ptr::NonNull;
use core::pin::Pin;
use core::future::Future;
use core::task::{Waker, Poll, Context};
use crate::chunk_slab::ChunkSlab;


type TaskKey = usize;

pub(crate) struct SchedulerAlgorithm{
    runnable0: VecDeque<TaskKey>,
    runnable1: VecDeque<TaskKey>,
    deferred: Vec<TaskKey>,
    last_waker: Arc<AtomicWakerRegistry>,
    registry: ChunkSlab<TaskKey,DynamicFuture>,
    current: Option<TaskKey>,
    which_buffer: bool,
}

impl SchedulerAlgorithm{
    pub(crate) fn new()->Self{
        Self{
            runnable0: VecDeque::new(),
            runnable1: VecDeque::new(),
            which_buffer: false,
            deferred: Vec::new(),
            last_waker: Arc::new(AtomicWakerRegistry::empty()),
            current: None,
            registry: ChunkSlab::new(),
        }
    }
    fn enqueue_runnable(&mut self,key: TaskKey){
        if self.which_buffer { //if now 1 is executed then add to 0 and vice versa.
            self.runnable0.push_back(key);
        } else {
            self.runnable1.push_back(key);
        }
    }

    pub(crate) fn register(&mut self,future: Pin<Box<dyn Future<Output=()> + 'static>>,suspended: bool)->TaskKey{
        let dynamic = DynamicFuture::new_allocated(future,self.last_waker.clone(),suspended);
        let key = self.registry.insert(dynamic); //won't realloc because it uses ChunkSlab
        //let key = self.new_tasks.push(dynamic); // we dont have task key yet...
        if !suspended {
            self.enqueue_runnable(key);
        }
        key
    }

    pub(crate) fn resume(&mut self,key: TaskKey)->bool{
        let task = self.registry.get_mut(key).unwrap();
        if task.is_suspended() {
            task.set_suspended(false);
            self.enqueue_runnable(key);
            return true;
        }
        false
    }

    pub(crate) fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop{
            self.last_waker.clear();//drop previous waker if any
            if self.which_buffer {
                if Self::beat_once(&mut self.registry,&mut self.runnable1,&mut self.runnable0,
                                   &mut self.deferred,cx.waker(),&self.last_waker,&mut self.current) {
                    return Poll::Pending;
                }
            }else{
                if Self::beat_once(&mut self.registry,&mut self.runnable0,&mut self.runnable1,
                                   &mut self.deferred,cx.waker(),&self.last_waker,&mut self.current) {
                    return Poll::Pending;
                }
            }
            self.which_buffer = !self.which_buffer;
            if self.runnable0.is_empty() && self.runnable1.is_empty() && self.deferred.is_empty() {break;}
        }
        Poll::Ready(()) //all tasks executed to finish
    }

    fn beat_once(registry: &mut ChunkSlab<TaskKey,DynamicFuture>,
                 from: &mut VecDeque<TaskKey>,to: &mut VecDeque<TaskKey>,
                 deferred: &mut Vec<TaskKey>,waker: &Waker,
                 last_waker: &AtomicWakerRegistry,current: &mut Option<TaskKey>)->bool{

        if !deferred.is_empty() && !Self::drain_runnable(registry,deferred,from){
            if from.is_empty() { //if has no work to do
                //no runnable task found, register waker
                last_waker.register(waker.clone());
                //check once again if no task was woken during this time
                if !Self::drain_runnable(registry,deferred,from) {
                    //waiting begins
                    return true;//true means that future should wait for waker
                }
                //if any was woken then try to deregister waker, then make one rotation
                last_waker.clear();
            }
        }
        Self::rotate_once(registry,from,to,deferred,current);
        false //can start new iteration
    }

    fn rotate_once(registry: &mut ChunkSlab<TaskKey,DynamicFuture>,from: &mut VecDeque<TaskKey>,
                   to: &mut VecDeque<TaskKey>, deferred: &mut Vec<TaskKey>,
                   current: &mut Option<TaskKey>){
        struct Guard<'a>(&'a mut Option<TaskKey>);
        impl<'a> Drop for Guard<'a>{ fn drop(&mut self) { *self.0 = None; } }

        while let Some(run_key) = from.pop_front() {
            let run_task = registry.get_mut(run_key).unwrap();
            *current = Some(run_key);
            let guard = Guard(current);
            // be careful with interior mutability types here cause poll_local can invoke any method
            // on handle, therefore 'from' queue shouldn't be edited by handles (other structures
            // are pretty much ok, actually even 'from' queue is ok with this cause it is not
            // borrowed by iterators or other such things but it might disturb task processing)
            let result = run_task.poll_local().is_pending(); //run user code
            drop(guard);

            if result { //reconsider enqueuing this future again
                if run_task.is_runnable() { //if immediately became runnable then enqueue it
                    to.push_back(run_key);
                } else { //put it on deferred queue
                    deferred.push(run_key);
                }
            }
        }
    }

    fn drain_runnable(registry: &mut ChunkSlab<TaskKey,DynamicFuture>,
                      from: &mut Vec<TaskKey>,to: &mut VecDeque<TaskKey>)->bool{
        let prev = from.len();
        from.retain(|elem|{
            let task = registry.get_mut(*elem).unwrap();
            if task.is_runnable(){
                to.push_back(*elem);
                return false;
            }
            true
        });
        prev != from.len()
    }
}