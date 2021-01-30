use alloc::boxed::Box;
use alloc::rc::Weak;
use alloc::string::{String, ToString};
use alloc::borrow::Cow;
use core::fmt::{Debug, Formatter};
use core::future::Future;
use core::hash::{Hash, Hasher};
use core::pin::Pin;
use crate::round::dyn_future::{DynamicFuture, TaskName};
use crate::round::Algorithm;

/// Handle used to spawn and control tasks in assigned [`Wheel`](struct.Wheel.html). All tasks
/// manipulation are done by this struct.
#[derive(Clone)]
pub struct WheelHandle<'futures> {
    ptr: Weak<Algorithm<'futures>>,
}

/// Represents identifier of task registered by [`WheelHandle`](struct.WheelHandle.html).
///
/// Identifiers are only valid when distinguishing tasks registered inside the same
/// [`Wheel`](struct.Wheel.html). Two different wheels can have tasks with the same identifiers.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IdNum(core::num::NonZeroUsize);

impl IdNum {
    fn from_usize(v: usize) -> Self {
        //SAFETY: any value + 1 is non-zero, except usize::MAX which should panic in debug mode
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
    pub(crate) suspended: bool,
    pub(crate) name: TaskName,
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
    ($s:expr,$ret:expr) => {
        match $s.ptr.upgrade() {
            Some(v) => v,
            None => return $ret,
        }
    }
}

impl<'futures> WheelHandle<'futures> {
    pub(crate) fn new(ptr: Weak<Algorithm<'futures>>) -> Self { Self { ptr } }


    /// Checks if this handle is valid. Handles are weak references bound to specific
    /// [`Wheel`](struct.Wheel.html).
    ///
    /// This method returns true if specific handle is valid. If handle is valid
    /// it means that it can be used to control tasks in `Wheel` associated with
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

    /// Checks if this and other handle reference the same [`Wheel`](struct.Wheel.html).
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

    /// Create new task and obtain its id, equivalent to: `spawn(SpawnParams::default(), future)`.
    /// # Arguments
    /// * `future` - The future you want to schedule.
    ///
    /// Allocates new task inside associated [`Wheel`](struct.Wheel.html). Returns identifier of newly
    /// allocated task or `None` if this handle is [`invalid`](#method.is_valid).
    pub fn spawn_default<F>(&self, future: F) -> Option<IdNum> where F: Future<Output=()> + 'futures {
        self.spawn_dyn(SpawnParams::default(), Box::pin(future))
    }
    /// Create new task and obtain its id.
    ///
    /// # Arguments
    /// * `params` - Task creation parameters. Using default will spawn runnable task without name.
    /// * `future` - The future you want to schedule.
    ///
    /// Allocates new task inside associated [`Wheel`](struct.Wheel.html). You can specify creation
    /// parameters of this task. Returns identifier of newly allocated task or None if this
    /// handle is [`invalid`](#method.is_valid).
    pub fn spawn<P,F>(&self, params: P, future: F) -> Option<IdNum>
        where F: Future<Output=()> + 'futures, P: Into<SpawnParams> {
        self.spawn_dyn(params, Box::pin(future))
    }

    /// Create new task from boxed future and obtain its id.
    ///
    /// # Arguments
    /// * `params` - Task creation parameters. Using default will spawn runnable task without name.
    /// * `future` - Boxed future you want to schedule.
    ///
    /// Allocates new task inside associated [`Wheel`](struct.Wheel.html). You can specify creation
    /// parameters of this task. Returns identifier of newly allocated task or None if this
    /// handle is [`invalid`](#method.is_valid).
    pub fn spawn_dyn<P>(&self, params: P, future: Pin<Box<dyn Future<Output=()> + 'futures>>) -> Option<IdNum>
        where P: Into<SpawnParams>{
        let this = unwrap_weak!(self,None);
        let dynamic = DynamicFuture::new(future, this.clone_registry(), params.into());
        this.register(dynamic).and_then(|v|Some(IdNum::from_usize(v)))
    }

    /// Cancel task with given id.
    ///
    /// If task is already executing then it will become cancelled when next yield occurs. Note that when
    /// getting [state](#method.get_state) of this cancelled task it might be `Cancelled` for some time
    /// but eventually it will become `Unknown` because scheduler removes tasks only after circling
    /// through all runnable tasks. Then when new task is spawned it might be assigned to the same id.
    pub fn cancel(&self, id: IdNum) -> bool {
        let this = unwrap_weak!(self,false);
        this.cancel(id.to_usize())
    }
    /// Suspend task with given id.
    ///
    /// Suspended tasks cannot execute until resumed. After this operation the task will change
    /// state to `Suspended`. Returns true if task was successfully suspended and false if it can't
    /// be suspended for any reason (e.g it was already suspended, cancelled or handle is [`invalid`](#method.is_valid)).
    pub fn suspend(&self, id: IdNum) -> bool {
        let this = unwrap_weak!(self,false);
        this.suspend(id.to_usize())
    }
    /// Resume task with given id.
    ///
    /// After this operation given task can be executed again. It will change state to `Runnable`
    /// or `Waiting` in case it still waits for some external event. Returns true if task was
    /// successfully resumed, and false if it wasn't suspended or handle is [`invalid`](#method.is_valid).
    pub fn resume(&self, id: IdNum) -> bool {
        let this = unwrap_weak!(self,false);
        this.resume(id.to_usize())
    }
    /// Get state of task with given id.
    ///
    /// If this handle is [`invalid`](#method.is_valid) then returns
    /// [`State::Unknown`](enum.State.html#variant.Unknown).
    /// For more information about allowed states see [`State`](enum.State.html).
    ///
    /// # Examples
    /// ```
    /// use juggle::*;
    ///
    /// let wheel = Wheel::new();
    /// let id1 = wheel.handle().spawn(SpawnParams::default(),async {/*...*/}).unwrap();
    /// let id2 = wheel.handle().spawn(SpawnParams::suspended(true),async {/*...*/}).unwrap();
    ///
    /// assert_eq!(wheel.handle().get_state(id1),State::Runnable);
    /// assert_eq!(wheel.handle().get_state(id2),State::Suspended);
    /// ```
    pub fn get_state(&self, id: IdNum) -> State {
        let this = unwrap_weak!(self,State::Unknown);
        this.get_state(id.to_usize())
    }
    /// Get id of currently executing task.
    ///
    /// Returns `None` when:
    /// * Not called inside task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// # Examples
    /// ```
    /// use juggle::*;
    ///
    /// async fn self_cancelling_task(handle: WheelHandle<'_>){
    ///     let id = handle.current().unwrap();
    ///     handle.cancel(id);
    ///     yield_once!(); // give control to scheduler
    ///
    ///     unreachable!("This line will never be reached.");
    /// }
    ///
    /// let wheel = Wheel::new();
    /// let handle = wheel.handle();
    /// handle.spawn(SpawnParams::default(), self_cancelling_task(handle.clone())).unwrap();
    /// smol::block_on(wheel).unwrap();
    /// ```
    pub fn current(&self) -> Option<IdNum> {
        let this = unwrap_weak!(self,None);
        this.get_current().map(|t| IdNum::from_usize(t))
    }

    /// Applies given function on reference to given task name and returns result of that function.
    ///
    /// The argument that is passed to `func` is `None` when:
    /// * Task is unnamed.
    /// * Given id is not assigned to any task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// # Examples
    /// ```
    /// use juggle::*;
    ///
    /// let wheel = Wheel::new();
    /// let bark = wheel.handle().spawn(SpawnParams::named("Dog"), async {/*...*/}).unwrap();
    /// let meow = wheel.handle().spawn(SpawnParams::named("Cat"), async {/*...*/}).unwrap();
    /// let none = wheel.handle().spawn(SpawnParams::default(), async {/*...*/}).unwrap();
    ///
    /// assert!(wheel.handle().with_name(bark, |name| name == Some("Dog")));
    /// assert!(wheel.handle().with_name(meow, |name| name == Some("Cat")));
    /// assert!(wheel.handle().with_name(none, |name| name == None));
    /// ```
    ///
    pub fn with_name<F, T>(&self, id: IdNum, func: F) -> T where F: FnOnce(Option<&str>) -> T {
        let this = unwrap_weak!(self,func(None));
        this.with_name(id.to_usize(),move |name|func(name.as_str()))
    }
    /// Returns name of current task.
    ///
    /// Returns `None` when:
    /// * Task is unnamed.
    /// * Not called inside task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// Note that if current task was named with static lifetime string, it will be returned from
    /// this method as `Cow::Borrowed`. When task has dynamic name then `Cow::Owned` is returned
    /// with fresh allocated `String` of that name. If you don't want to allocate temporary memory
    /// for string use in such case, use [`with_name`](#method.with_name).
    pub fn get_current_name(&self) -> Option<Cow<'static,str>> {
        self.current().and_then(|id| self.get_name(id))
    }
    /// Returns name of task with given id.
    /// Returns `None` when:
    /// * Task is unnamed.
    /// * Given id is not assigned to any task.
    /// * Handle is [`invalid`](#method.is_valid).
    ///
    /// Note that if specified task was named with static lifetime string, it will be returned from
    /// this method as `Cow::Borrowed`. When task has dynamic name then `Cow::Owned` is returned
    /// with fresh allocated `String` of that name. If you don't want to allocate temporary memory
    /// for string use in such case, use [`with_name`](#method.with_name).
    pub fn get_name(&self, id: IdNum) -> Option<Cow<'static,str>> {
        let this = unwrap_weak!(self,None);
        this.with_name(id.to_usize(),move |name|{
            match name {
                TaskName::Static(s) => Some(Cow::<'static,str>::Borrowed(s)),
                TaskName::Dynamic(s) => Some(Cow::<'static,str>::Owned(s.to_string())),
                TaskName::None => None,
            }
        })
    }
    /// Find task id that has name equal to given argument.
    ///
    /// Returns `None` when:
    /// * Task was not found.
    /// * Handle is [`invalid`](#method.is_valid).
    pub fn get_by_name(&self, name: &str) -> Option<IdNum> {
        let this = unwrap_weak!(self,None);
        this.get_by_name(name).map(|k| IdNum::from_usize(k))
    }

    /// Returns number of total registered tasks in this scheduler at the moment or 0 if this handle
    /// is [`invalid`](#method.is_valid).
    pub fn registered_count(&self)->usize{
        let this = unwrap_weak!(self,0);
        this.registered_count()
    }

    fn fmt_name(&self, f: &mut Formatter<'_>, name: &str) -> core::fmt::Result {
        let this = unwrap_weak!(self,write!(f,"{}{{ Invalid }}",name));
        this.format_internal(f, name)
    }
}

impl<'futures> Debug for WheelHandle<'futures> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.fmt_name(f, "WheelHandle")
    }
}

impl<'futures> PartialEq for WheelHandle<'futures> {
    fn eq(&self, other: &Self) -> bool { self.is_same(other) }
}
impl<'futures> Eq for WheelHandle<'futures> {}

impl<'futures> Hash for WheelHandle<'futures> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.ptr.as_ptr() as usize); // identity hash code
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
        if let Some(s) = self.name.as_str() {
            write!(f, "SpawnParams[name: \"{}\", suspended: {}]", s, self.suspended)
        } else {
            write!(f, "SpawnParams[suspended: {}]", self.suspended)
        }
    }
}

impl From<&'static str> for SpawnParams {
    /// Works as [`named`](struct.SpawnParams.html#method.named) method.
    fn from(v: &'static str) -> Self { Self::named(v) }
}

impl From<String> for SpawnParams {
    /// Works as [`dyn_named`](struct.SpawnParams.html#method.dyn_named) method.
    fn from(v: String) -> Self { Self::dyn_named(v) }
}

impl From<bool> for SpawnParams {
    /// Works as [`suspended`](struct.SpawnParams.html#method.suspended) method.
    fn from(v: bool) -> Self { Self::suspended(v) }
}