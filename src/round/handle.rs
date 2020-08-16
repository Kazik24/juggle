use alloc::boxed::Box;
use alloc::rc::{Rc, Weak};
use alloc::string::{String, ToString};
use core::cell::UnsafeCell;
use core::fmt::{Debug, Formatter};
use core::future::Future;
use core::pin::Pin;
use crate::round::algorithm::SchedulerAlgorithm;
use crate::round::dyn_future::{DynamicFuture, TaskName};

/// Handle used to spawn and control tasks in assigned [Wheel](struct.Wheel.html).
#[derive(Clone)]
pub struct WheelHandle<'futures> {
    ptr: Weak<UnsafeCell<SchedulerAlgorithm<'futures>>>,
}

/// Represents identifier of task registered by [WheelHandle](struct.WheelHandle.html).
///
/// Identifiers are only valid when distinguishing tasks registered inside the same
/// [Wheel](struct.Wheel.html). Two different wheels can have tasks with the same identifiers.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IdNum(core::num::NonZeroUsize);

impl IdNum {
    fn from_usize(v: usize) -> Self {
        Self(unsafe { core::num::NonZeroUsize::new_unchecked(v + 1) })
    }
    fn to_usize(self) -> usize {
        self.0.get() - 1
    }
}

impl Debug for IdNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "IdNum[0x{:X}]", self.to_usize())
    }
}

/// Parameters used when spawning task inside scheduler.
///
/// Default parameters will spawn runnable unnamed task.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct SpawnParams {
    suspended: bool,
    name: TaskName,
}

/// Represents state of a task.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
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
    /// Given key has no associated task with it, handle used to obtain state was invalid or task
    /// has completed and was removed from scheduler.
    Unknown,
}

macro_rules! unwrap_weak {
    ($s:expr,$this:ident,$ret:expr) => {
        let rc = match $s.ptr.upgrade() {
            Some(v) => v,
            None => return $ret,
        };
        let $this = Self::unchecked_mut(&rc);
    }
}

impl<'futures> WheelHandle<'futures> {
    pub(crate) fn new(ptr: Weak<UnsafeCell<SchedulerAlgorithm<'futures>>>) -> Self { Self { ptr } }


    /// Checks if this handle is valid.
    ///
    /// This method returns true if specific handle is valid. If handle is valid
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
    pub fn is_valid(&self) -> bool { self.ptr.strong_count() != 0 }

    /// Checks if this and the other handle reference the same [Wheel](struct.Wheel.html).
    /// # Examples
    /// ```
    /// use juggle::*;
    /// let wheel = Wheel::new();
    /// let wheel2 = Wheel::new();
    ///
    /// let h1 = wheel.handle();
    /// let h2 = h1.clone();
    /// let other = wheel2.handle();
    ///
    /// assert!(h1.is_same(&h2));
    /// assert!(!h1.is_same(&other));
    /// ```
    pub fn is_same(&self, other: &WheelHandle<'_>) -> bool {
        let ptr1 = self.ptr.as_ptr() as *const ();
        let ptr2 = other.ptr.as_ptr() as *const ();
        ptr1 == ptr2
    }

    /// Create new task and obtain its id.
    ///
    /// # Arguments
    /// * `params` - Task creation parameters. Using default will spawn runnable task without name.
    /// * `future` - The future you want to schedule.
    ///
    /// Allocates new task inside associated [Wheel](struct.Wheel.html). You can specify creation
    /// parameters of this task. Returns identifier of newly allocated task or None if this
    /// handle is invalid.
    pub fn spawn<F>(&self, params: impl Into<SpawnParams>, future: F) -> Option<IdNum> where F: Future<Output=()> + 'futures {
        self.spawn_dyn(params, Box::pin(future))
    }

    /// Create new task from boxed future and obtain its id.
    ///
    /// # Arguments
    /// * `params` - Task creation parameters. Using default will spawn runnable task without name.
    /// * `future` - Boxed future you want to schedule.
    ///
    /// Allocates new task inside associated [Wheel](struct.Wheel.html). You can specify creation
    /// parameters of this task. Returns identifier of newly allocated task or None if this
    /// handle is invalid.
    pub fn spawn_dyn(&self, params: impl Into<SpawnParams>, future: Pin<Box<dyn Future<Output=()> + 'futures>>) -> Option<IdNum> {
        unwrap_weak!(self,this,None);
        let params = params.into();
        let mut dynamic = DynamicFuture::new_allocated(future, this.clone_registry(), params.suspended);
        dynamic.set_name(params.name);
        Some(IdNum::from_usize(this.register(dynamic) as usize))
    }

    /// Cancel task with given id.
    ///
    /// If task is already executing then it will be removed when next yield occurs. Note that when
    /// getting [state](#method.get_state) of this cancelled task it might be `Cancelled` for some time
    /// but eventually it will become `Unknown` because scheduler removes tasks only after circling
    /// through all runnable tasks. Then when new task is spawned it might be assigned to the same id.
    pub fn cancel(&self, id: IdNum) -> bool {
        unwrap_weak!(self,this,false);
        this.cancel(id.to_usize())
    }
    /// Suspend task with given id.
    ///
    /// Suspended tasks cannot execute until resumed.
    pub fn suspend(&self, id: IdNum) -> bool {
        unwrap_weak!(self,this,false);
        this.suspend(id.to_usize())
    }
    /// Resume task with given id.
    ///
    /// Makes this task `Runnable` again.
    pub fn resume(&self, id: IdNum) -> bool {
        unwrap_weak!(self,this,false);
        this.resume(id.to_usize())
    }
    /// Get state of task with given id.
    ///
    /// If this handle is [`invalid`](#method.is_valid) then returns
    /// [`State::Unknown`](enum.State.html#variant.Unknown).
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
        this.get_state(id.to_usize())
    }
    /// Get id of currently executing task.
    ///
    /// Returns None when:
    /// * Not called inside task.
    /// * Handle is [`invalid`](#method.is_valid).
    pub fn current(&self) -> Option<IdNum> {
        unwrap_weak!(self,this,None);
        this.get_current().map(|t| IdNum::from_usize(t))
    }

    /// Applies given function on reference to given task name.
    ///
    /// Function argument is a name of task with specified id or None if task has no name, given
    /// id has no assigned task or this handle is invalid. Returns result of function call.
    pub fn with_name<F, T>(&self, id: IdNum, func: F) -> T where F: FnOnce(Option<&str>) -> T {
        unwrap_weak!(self,this,func(None));
        let arg = this.get_dynamic(id.to_usize()).map(|v| v.get_name_str()).flatten();
        func(arg)
    }
    /// Returns name of current task as new String.
    /// Returns None when:
    /// * Task is unnamed.
    /// * This method was not called inside task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// Note that this method allocates new string. If you don't want to allocate temporary memory
    /// for string use [`with_name`](#method.with_name).
    pub fn get_current_name(&self) -> Option<String> {
        self.current().map(|id| self.get_name(id)).flatten()
    }
    /// Returns name of task with specific id as new String.
    /// Returns None when:
    /// * Task is unnamed.
    /// * Given id is not assigned to any task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// Note that this method allocates new string. If you don't want to allocate temporary memory
    /// for string use [`with_name`](#method.with_name).
    pub fn get_name(&self, id: IdNum) -> Option<String> {
        self.with_name(id, |s| s.map(|s| s.to_string()))
    }
    /// Find task id that has name equal to given argument.
    ///
    /// Returns None when:
    /// * Task was not found.
    /// * Handle is [`invalid`](#method.is_valid).
    pub fn get_by_name(&self, name: &str) -> Option<IdNum> {
        unwrap_weak!(self,this,None);
        this.get_by_name(name).map(|k| IdNum::from_usize(k))
    }

    fn unchecked_mut<'a>(rc: &'a Rc<UnsafeCell<SchedulerAlgorithm<'futures>>>) -> &'a mut SchedulerAlgorithm<'futures> {
        unsafe { &mut *rc.get() }
    }

    fn fmt_name(&self, f: &mut Formatter<'_>, name: &str) -> core::fmt::Result {
        unwrap_weak!(self,this,write!(f,"{}{{ Invalid }}",name));
        this.format_internal(f, name)
    }
}

impl<'futures> Debug for WheelHandle<'futures> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.fmt_name(f, "WheelHandle")
    }
}

impl SpawnParams {
    /// Set suspended property.
    pub fn suspend(mut self, value: bool) -> Self {
        self.suspended = value;
        self
    }
    /// Set name property to static string slice.
    pub fn name(mut self, name: &'static str) -> Self {
        self.name = TaskName::Static(name);
        self
    }
    /// Set name property to dynamically allocated string.
    pub fn dyn_name(mut self, name: impl Into<String>) -> Self {
        self.name = TaskName::Dynamic(name.into().into_boxed_str());
        self
    }
    /// Create default parameters (suspended property set to false) with name property set to
    /// static string slice.
    pub fn named(name: &'static str) -> Self { Self::default().name(name) }
    /// Create default parameters (suspended property set to false) with name property set to
    /// dynamically allocated string.
    pub fn dyn_named(name: impl Into<String>) -> Self { Self::default().dyn_name(name) }
    /// Create default parameters (name property set to none) with suspended property set to given
    /// value.
    pub fn suspended(value: bool) -> Self { Self::default().suspend(value) }
}

impl Default for SpawnParams {
    /// Create default parameters that will spawn runnable unnamed task.
    fn default() -> Self {
        Self {
            suspended: false,
            name: TaskName::None,
        }
    }
}

impl Debug for SpawnParams {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let s: Option<&str> = match &self.name {
            TaskName::None => None,
            TaskName::Dynamic(s) => Some(&s),
            TaskName::Static(s) => Some(s),
        };
        if let Some(s) = s {
            write!(f, "SpawnParams[name: \"{}\", suspended: {}]", s, self.suspended)
        } else {
            write!(f, "SpawnParams[suspended: {}]", self.suspended)
        }
    }
}

impl From<&'static str> for SpawnParams {
    /// Works as [named](struct.SpawnParams.html#method.named) method.
    fn from(v: &'static str) -> Self { SpawnParams::named(v) }
}

impl From<String> for SpawnParams {
    /// Works as [dyn_named](struct.SpawnParams.html#method.dyn_named) method.
    fn from(v: String) -> Self { SpawnParams::dyn_named(v) }
}

impl From<bool> for SpawnParams {
    /// Works as [suspended](struct.SpawnParams.html#method.suspended) method.
    fn from(v: bool) -> Self { SpawnParams::suspended(v) }
}