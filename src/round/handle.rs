use alloc::rc::{Weak, Rc};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use core::cell::UnsafeCell;
use crate::round::algorithm::SchedulerAlgorithm;
use core::future::Future;
use core::pin::Pin;
use crate::round::dyn_future::{DynamicFuture, TaskName};
use core::fmt::{Debug, Formatter};

#[derive(Clone)]
pub struct WheelHandle<'futures>{
    ptr: Weak<UnsafeCell<SchedulerAlgorithm<'futures>>>,
}

#[derive(Copy,Clone,Eq,PartialEq,Ord,PartialOrd,Hash)]
pub struct IdNum(usize);
impl Debug for IdNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f,"TaskId[0x{:X}]",self.0)
    }
}
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub struct SpawnParams {
    suspended: bool,
    name: TaskName,
}

/// Represents state of a task.
#[derive(Copy,Clone,Eq,PartialEq,Ord,PartialOrd,Hash,Debug)]
#[repr(u8)]
pub enum State {
    /// Task is currently executing or waiting for its turn to execute.
    Runnable,
    /// Task is suspended and will not execute until resumed.
    Suspended,
    /// Task is waiting for external event to wake it.
    Waiting,
    /// Task is cancelled but was not removed form scheduler yet.
    Cancelled,
    /// Given key has no associated task with it, or handle used to obtain state was invalid.
    Unknown,
}

macro_rules! unwrap_weak{
    ($s:expr,$this:ident,$ret:expr) => {
        let rc = match $s.ptr.upgrade() {
            Some(v) => v,
            None => return $ret,
        };
        let $this = Self::unchecked_mut(&rc);
    }
}

impl<'futures> WheelHandle<'futures>{
    pub(crate) fn new(ptr: Weak<UnsafeCell<SchedulerAlgorithm<'futures>>>)->Self{ Self{ptr} }


    /// Checks if this handle is valid.
    ///
    /// This method returns true if specific handle is valid and false otherwise. If handle is valid
    /// it means that it can be used to control tasks in [Wheel](struct.Wheel.html) associated with
    /// it. Handle is valid until associated wheel is dropped or
    /// [`locked`](struct.Wheel.html#method.lock).
    ///
    /// # Examples
    /// ```
    /// use juggle::*;
    /// let wheel = Wheel::new();
    /// let handle = wheel.handle().clone();
    ///
    /// assert!(handle.is_valid()); // Ok, useful handle.
    /// drop(wheel);
    /// assert!(!handle.is_valid()); // Handle can no longer be used to control Wheel.
    /// ```
    pub fn is_valid(&self)->bool{ self.ptr.strong_count() != 0 }

    /// Create new task and obtain its id.
    ///
    /// # Arguments
    /// * `params` - Task creation parameters. Using default will spawn runnable task without name.
    /// * `future` - The future you want to schedule.
    /// Allocates new task inside associated [Wheel](struct.Wheel.html). You can specify creation
    /// parameters of this task.
    ///
    /// Returns id of newly allocated task or None if this handle is invalid.
    pub fn spawn<F>(&self, params: impl Into<SpawnParams>, future: F) ->Option<IdNum> where F: Future<Output=()> + 'futures{
        self.spawn_dyn(params,Box::pin(future))
    }

    pub fn spawn_dyn(&self, params: impl Into<SpawnParams>, future: Pin<Box<dyn Future<Output=()> + 'futures>>) ->Option<IdNum>{
        unwrap_weak!(self,this,None);
        let params = params.into();
        let mut dynamic = DynamicFuture::new_allocated(future,this.clone_registry(),params.suspended);
        dynamic.set_name(params.name);
        Some(IdNum(this.register(dynamic) as usize))
    }

    /// Cancel task with given id.
    ///
    /// If task is already executing then it will remove if when next yield occurs.
    pub fn cancel(&self, id: IdNum) ->bool{
        unwrap_weak!(self,this,false);
        this.cancel(id.0)
    }
    /// Suspend task with given id.
    pub fn suspend(&self, id: IdNum) ->bool{
        unwrap_weak!(self,this,false);
        this.suspend(id.0)
    }
    /// Resume task with given id.
    pub fn resume(&self, id: IdNum) ->bool{
        unwrap_weak!(self,this,false);
        this.resume(id.0)
    }
    /// Returns state of task with given id.
    ///
    /// For more information about allowed states see [State](enum.State.html)
    ///
    /// # Examples
    /// ```
    /// use juggle::*;
    ///
    /// let wheel = Wheel::new();
    /// let id1 = wheel.handle().spawn(SpawnParams::default(),async move {/*...*/}).unwrap();
    /// let id2 = wheel.handle().spawn(SpawnParams::suspended(true),async move {/*...*/}).unwrap();
    ///
    /// assert_eq!(wheel.handle().get_state(id1),State::Runnable);
    /// assert_eq!(wheel.handle().get_state(id2),State::Suspended);
    /// ```
    pub fn get_state(&self, id: IdNum) -> State {
        unwrap_weak!(self,this,State::Unknown);
        this.get_state(id.0)
    }
    /// Returns id of currently executing task, or None if not called inside task or this handle is
    /// invalid.
    pub fn current(&self) ->Option<IdNum>{
        unwrap_weak!(self,this,None);
        this.get_current().map(|t| IdNum(t))
    }
    pub fn with_name<F,T>(&self, id: IdNum, func: F) ->T where F: FnOnce(Option<&str>)->T{
        unwrap_weak!(self,this,func(None));
        match this.get_dynamic(id.0) {
            Some(v) => match v.get_name() {
                TaskName::Static(s) => func(Some(s)),
                TaskName::Dynamic(d) => func(Some(&d)),
                TaskName::None => func(None),
            }
            None => func(None),
        }
    }
    pub fn get_current_name(&self)->Option<String>{
        self.current().map(|id|self.with_name(id, |s|s.map(|s|s.to_string()).unwrap_or(String::new())))
    }
    pub fn get_by_name(&self,name: &str)->Option<IdNum>{
        unwrap_weak!(self,this,None);
        this.get_by_name(name).map(|k|IdNum(k))
    }


    pub fn terminate_scheduler(&self)->bool{//remove all tasks from scheduler
        todo!()
    }

    fn unchecked_mut<'a>(rc: &'a Rc<UnsafeCell<SchedulerAlgorithm<'futures>>>)->&'a mut SchedulerAlgorithm<'futures>{
        unsafe{ &mut *rc.get() }
    }

    fn fmt_name(&self, f: &mut Formatter<'_>,name: &str) -> core::fmt::Result {
        let rc = match self.ptr.upgrade() {
            Some(v) => v,
            None => return write!(f,"{}{{ Invalid }}",name),
        };
        let this = Self::unchecked_mut(&rc);
        this.format_internal(f,name)
    }
}

impl<'futures> Debug for WheelHandle<'futures>{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.fmt_name(f,"WheelHandle")
    }
}

impl SpawnParams {
    pub fn suspend(mut self,value: bool)->Self{
        self.suspended = value;
        self
    }
    pub fn name(mut self,name: &'static str)->Self{
        self.name = TaskName::Static(name);
        self
    }
    pub fn dyn_name(mut self,name: impl Into<String>)->Self{
        self.name = TaskName::Dynamic(name.into().into_boxed_str());
        self
    }
    pub fn named(name: &'static str)->Self{Self::default().name(name)}
    pub fn dyn_named(name: impl Into<String>)->Self{Self::default().dyn_name(name)}
    pub fn suspended(value: bool)->Self{Self::default().suspend(value)}
}
impl Default for SpawnParams {
    fn default() -> Self {
        Self{
            suspended: false,
            name: TaskName::None,
        }
    }
}
impl From<&'static str> for SpawnParams{
    fn from(v: &'static str) -> Self {SpawnParams::named(v)}
}
impl From<String> for SpawnParams {
    fn from(v: String) -> Self {SpawnParams::dyn_named(v)}
}
impl From<bool> for SpawnParams{
    fn from(v: bool) -> Self {SpawnParams::suspended(v)}
}