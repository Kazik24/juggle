use crate::round::dyn_future::{DynamicFuture, TaskName};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::utils::AtomicWakerRegistry;
use core::task::{Waker, Poll, Context};
use crate::chunk_slab::ChunkSlab;
use crate::round::handle::State;
use std::ops::Deref;
use std::fmt::{Formatter, Debug, UpperHex};
use std::mem::swap;
use core::fmt::Result;
use crate::WheelHandle;


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
    pub(crate) fn get_current(&self)->Option<TaskKey>{self.current}
    //safe to call from inside task
    fn enqueue_runnable(&mut self,key: TaskKey){
        if self.which_buffer { //if now 1 is executed then add to 0 and vice versa.
            self.runnable0.push_back(key);
        } else {
            self.runnable1.push_back(key);
        }
    }
    pub(crate) fn clone_registry(&self)->Arc<AtomicWakerRegistry>{self.last_waker.clone()}
    //safe to call from inside task
    pub(crate) fn register(&mut self,dynamic: DynamicFuture)->TaskKey{
        let suspended = dynamic.is_suspended();
        let key = self.registry.insert(dynamic); //won't realloc because it uses ChunkSlab
        if !suspended {
            self.enqueue_runnable(key);
        }
        key
    }
    //safe to call from inside task
    pub(crate) fn resume(&mut self,key: TaskKey)->bool{
        match self.registry.get_mut(key) {
            Some(task) if task.is_suspended() => {
                task.set_suspended(false);
                self.enqueue_runnable(key); //suspended task always has runnable state (for now)
                true
            }
            _ => false,
        }
    }

    //if rotate_once encounters suspended task, then it will be removed from queue
    pub(crate) fn suspend(&mut self,key: TaskKey)->bool{
        match self.registry.get_mut(key) {
            Some(task) => {
                let prev = task.is_suspended();
                task.set_suspended(true);
                !prev
            }
            None => false,
        }
    }

    pub(crate) fn get_state(&self,key: TaskKey)-> State {
        match self.registry.get(key) {
            Some(task) => {
                if task.is_cancelled() { State::Cancelled }
                else if task.is_suspended() { State::Suspended }
                else if task.is_runnable() { State::Runnable }
                else { State::Waiting }
            }
            None => State::Unknown,
        }
    }

    pub(crate) fn cancel(&mut self,key: TaskKey)->bool{
        match self.registry.get_mut(key) {
            Some(task) => {
                let prev = task.is_cancelled();
                task.set_cancelled(true);
                !prev
            }
            None => false,
        }
    }
    pub(crate) fn get_by_name(&self,name: &str)->Option<TaskKey>{
        for (k,v) in self.registry.iter(){
            match v.get_name() {
                TaskName::Dynamic(n) if n.deref() == name => return Some(k),
                TaskName::Static(n) if *n == name => return Some(k),
                _ => {}
            }
        }
        None
    }

    pub(crate) fn get_dynamic(&self,key: TaskKey)->Option<&DynamicFuture>{ self.registry.get(key) }
    pub(crate) fn get_dynamic_mut(&mut self,key: TaskKey)->Option<&mut DynamicFuture>{ self.registry.get_mut(key) }

    pub(crate) fn format_internal(&self, f: &mut Formatter<'_>,name: &str) -> Result {
        pub(crate) struct DebugTask<'a>(
            &'a ChunkSlab<TaskKey,DynamicFuture>,
            Option<TaskKey>,
        );

        impl<'a> Debug for DebugTask<'a>{
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self.1 {
                    Some(id) => {
                        if let Some(task) = self.0.get(id) {
                            return match task.get_name_str() {
                                Some(s) => write!(f,"0x{:X}:{}",id,s),
                                None => write!(f,"0x{:X}",id),
                            }
                        }
                    },
                    _ => {},
                }
                write!(f,"None")
            }
        }

        writeln!(f,"{}{{",name)?;
        let span = 10;
        writeln!(f,"{:>s$}: {:?}","current",DebugTask(&self.registry,self.current),s=span)?;

        struct RunnableDebug<'a>(&'a SchedulerAlgorithm);
        impl<'a> Debug for RunnableDebug<'a>{
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                if self.0.runnable0.is_empty() && self.0.runnable1.is_empty() { write!(f,"None") }
                else{
                    let mut buff0 = &self.0.runnable0;
                    let mut buff1 = &self.0.runnable1;
                    if self.0.which_buffer {swap(&mut buff0,&mut buff1);}
                    let buff0 = buff0.iter().map(|&k|DebugTask(&self.0.registry,Some(k)));
                    let buff1 = buff1.iter().map(|&k|DebugTask(&self.0.registry,Some(k)));
                    f.debug_list().entries(buff0).entries(buff1).finish()
                }
            }
        }
        writeln!(f,"{:>s$}: {:?}","runnable",RunnableDebug(self),s=span)?;

        struct WaitingDebug<'a>(&'a SchedulerAlgorithm);
        impl<'a> Debug for WaitingDebug<'a>{
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let buff = self.0.deferred.iter().map(|&k|DebugTask(&self.0.registry,Some(k)));
                if self.0.deferred.is_empty() { write!(f,"None") }
                else { f.debug_list().entries(buff).finish() }
            }
        }
        writeln!(f,"{:>s$}: {:?}","waiting",WaitingDebug(self),s=span)?;

        struct SuspendedDebug<'a>(&'a SchedulerAlgorithm);
        impl<'a> Debug for SuspendedDebug<'a>{
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let mut buff = self.0.registry.iter().map(|(k,_)|DebugTask(&self.0.registry,Some(k)))
                    .filter(|t|{
                        match t.1.map(|id|t.0.get(id)).flatten() {
                            Some(task) => task.is_suspended(),
                            None => false,
                        }
                    });
                if let Some(first) = buff.next() { f.debug_list().entry(&first).entries(buff).finish() }
                else { write!(f,"None") }
            }
        }
        writeln!(f,"{:>s$}: {:?}","suspended",SuspendedDebug(self),s=span)?;
        write!(f,"}}")
    }

    // TODO: what to do when all tasks are suspended.
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
            if run_task.is_cancelled() {
                registry.remove(run_key);
                continue; //remove from queue and registry
            }
            if run_task.is_suspended() {
                continue; // remove from queue
            }
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
        from.retain(|&elem|{
            let task = registry.get_mut(elem).unwrap();
            if task.is_runnable(){
                to.push_back(elem);
                return false;
            }
            true
        });
        prev != from.len()
    }
}