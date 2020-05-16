use std::rc::{Weak, Rc};
use std::cell::UnsafeCell;
use crate::round::algorithm::SchedulerAlgorithm;
use core::future::Future;
use core::task::*;
use std::pin::Pin;
use crate::round::dyn_future::{DynamicFuture, TaskName};

#[derive(Clone)]
pub struct WheelHandle{
    ptr: Weak<UnsafeCell<SchedulerAlgorithm>>,
}

pub struct TaskId(usize);

pub struct TaskParams{
    suspended: bool,
    name: TaskName,
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

impl WheelHandle{
    pub(crate) fn new(ptr: Weak<UnsafeCell<SchedulerAlgorithm>>)->Self{ Self{ptr} }
    pub fn is_valid(&self)->bool{ self.ptr.strong_count() != 0 }

    pub fn spawn<F>(&self,params: TaskParams,future: F)->Option<TaskId> where F: Future<Output=()> + 'static{
        unwrap_weak!(self,this,None);
        let mut dynamic = DynamicFuture::new_allocated(Box::pin(future),this.clone_registry(),params.suspended);
        dynamic.set_name(params.name);
        Some(TaskId(this.register(dynamic) as usize))
    }
    pub fn spawn_dyn(&self,params: TaskParams,future: Pin<Box<dyn Future<Output=()> + 'static>>)->Option<TaskId>{
        unwrap_weak!(self,this,None);
        let mut dynamic = DynamicFuture::new_allocated(Box::pin(future),this.clone_registry(),params.suspended);
        dynamic.set_name(params.name);
        Some(TaskId(this.register(dynamic) as usize))
    }

    pub fn cancel(&self,id: TaskId)->bool{
        unwrap_weak!(self,this,false);
        this.cancel(id.0)
    }
    pub fn suspend(&self,id: TaskId)->bool{
        unwrap_weak!(self,this,false);
        this.suspend(id.0)
    }
    pub fn current_task(&self)->Option<TaskId>{
        unwrap_weak!(self,this,None);
        this.get_current().map(|t|TaskId(t))
    }
    pub fn with_name<F,T>(&self,id: TaskId,func: F)->Option<T> where F: FnOnce(&str)->T{
        unwrap_weak!(self,this,None);
        let task: Option<&DynamicFuture> = this.get_dynamic(id.0);
        match task {
            Some(v) => match v.get_name() {
                TaskName::Static(s) => Some(func(s)),
                TaskName::Dynamic(d) => Some(func(&d)),
                TaskName::None => None,
            }
            None => None,
        }
    }
    pub fn get_current_name(&self)->Option<String>{
        self.current_task().map(|id|self.with_name(id,|s|s.to_string()).unwrap())
    }

    fn unchecked_mut(rc: &Rc<UnsafeCell<SchedulerAlgorithm>>)->&mut SchedulerAlgorithm{
        unsafe{ &mut *rc.get() }
    }

}

impl TaskParams{
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
impl Default for TaskParams{
    fn default() -> Self {
        Self{
            suspended: false,
            name: TaskName::None,
        }
    }
}