use crate::utils::ChunkSlab;
use crate::round::algorithm::TaskKey;
use crate::round::dyn_future::DynamicFuture;
use core::cell::*;
use core::ops::Deref;

/// ChunkSlab wrapper with interior mutability.
/// SAFETY: Intended only to use inside this crate.
/// Provides runtime borrow checking in debug mode and only wraps UnsafeCell without any
/// checks in release mode.
pub(crate) struct Registry<'future>{
    slab: UnsafeCell<ChunkSlab<TaskKey,DynamicFuture<'future>>>,
    #[cfg(debug_assertions)]
    borrow_flag: Cell<usize>,
    #[cfg(debug_assertions)]
    iterate_flag: Cell<usize>,
}

pub(crate) struct TaskRef<'a,'future>{
    inner: &'a DynamicFuture<'future>,
    #[cfg(debug_assertions)]
    borrow_flag: &'a Cell<usize>,
}

impl<'future> Deref for TaskRef<'_,'future>{
    type Target = DynamicFuture<'future>;
    #[inline(always)] fn deref(&self) -> &Self::Target { self.inner }
}

#[cfg(debug_assertions)]
impl Drop for TaskRef<'_,'_>{
    #[inline]
    fn drop(&mut self) {
        //decrement borrow count
        self.borrow_flag.set(self.borrow_flag.get()-1);
    }
}

impl<'future> Registry<'future>{
    #[cfg(debug_assertions)]
    const SENTINEL: usize = usize::MAX; //so that incrementing it would overflow

    #[inline]
    pub fn new()->Self{
        Self{
            slab: UnsafeCell::new(ChunkSlab::new()),
            #[cfg(debug_assertions)]
            borrow_flag: Cell::new(0),
            #[cfg(debug_assertions)]
            iterate_flag: Cell::new(0),
        }
    }
    #[inline]
    pub fn get<'a>(&'a self,key: TaskKey)->Option<TaskRef<'a,'future>>{
        #[cfg(debug_assertions)]
        if self.borrow_flag.get() == Self::SENTINEL {
            panic!("Registry: Cannot borrow task during retain operation.");
        }

        //SAFETY: borrow flag saves state of struct as borrowed
        match unsafe{ &*self.slab.get() }.get(key) {
            None => None, //no borrow needed
            Some(task) => {
                #[cfg(debug_assertions)]
                self.borrow_flag.set(self.borrow_flag.get() + 1); //indicate borrow
                Some(TaskRef{
                    inner: task,
                    #[cfg(debug_assertions)]
                    borrow_flag: &self.borrow_flag
                })
            }
        }
    }
    #[inline]
    pub fn insert(&self,val: DynamicFuture<'future>)->TaskKey{
        #[cfg(debug_assertions)]
        if self.iterate_flag.get() != 0 {
            if self.iterate_flag.get() == Self::SENTINEL{
                panic!("Registry: Cannot insert task during retain operation.");
            }else{
                panic!("Registry: Cannot insert task during iteration.");
            }
        }
        unsafe{
            //SAFETY: We know that chunk slab wont reallocate or change memory of already
            //borrowed tasks when inserting and we checked if any iterator is present
            let r = &mut *self.slab.get();
            r.insert(val)
        }
    }
    #[inline]
    pub fn remove(&self,key: TaskKey)->Option<DynamicFuture<'future>>{
        #[cfg(debug_assertions)]
        if self.borrow_flag.get() != 0 || self.iterate_flag.get() != 0 {
            if self.borrow_flag.get() == Self::SENTINEL {
                panic!("Registry: Cannot remove task during retain operation.");
            }else{
                panic!("Registry: Cannot remove task that might be borrowed.");
            }
        }
        //SAFETY: we just checked if anything is borrowed
        unsafe{ (&mut *self.slab.get()).remove(key) }
    }
    #[inline]
    pub fn count(&self)->usize{
        //SAFETY: this is always safe cause iterators and borrows cannot resize slab and resizing
        //operations such as insert or remove are not recursive (with exception for retain which
        //updates count after iterating all tasks).
        unsafe{ (&*self.slab.get()).len() }
    }

    #[cfg(debug_assertions)]
    #[inline(always)]
    fn guarded_iterator(&self) -> impl Iterator<Item=(TaskKey, &DynamicFuture<'future>)> {
        struct It<'a,T>(T,&'a Cell<usize>);
        impl<T> Drop for It<'_,T>{
            fn drop(&mut self) { self.1.set(self.1.get()-1); }
        }
        impl<'a,T: Iterator> Iterator for It<'a,T>{
            type Item = T::Item;
            fn next(&mut self) -> Option<Self::Item> { self.0.next() }
        }

        if self.iterate_flag.get() == Self::SENTINEL { //check if we're not retaining
            panic!("Registry: Cannot iterate during retain operation.");
        }

        self.iterate_flag.set(self.iterate_flag.get() + 1);//we will return borrow from this function

        //SAFETY we just inserted iterate flag into iterator, and it will decrement it on drop
        //so this reference is guarded
        let r = unsafe{ &*self.slab.get() };
        It(r.iter(),&self.iterate_flag)
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item=(TaskKey, &DynamicFuture<'future>)>{
        #[cfg(debug_assertions)]
        return self.guarded_iterator();
        #[cfg(not(debug_assertions))]
        return unsafe{ (&*self.slab.get()).iter() };
    }

    #[cfg(debug_assertions)]
    #[inline(always)]
    fn guard_retain<'b>(&'b self)->impl Drop + 'b{
        if self.borrow_flag.get() != 0 || self.iterate_flag.get() != 0 {
            panic!("Registry: Cannot remove tasks that might be borrowed.");
        }
        struct Guard<'a>(&'a Cell<usize>, &'a Cell<usize>);
        impl Drop for Guard<'_>{
            fn drop(&mut self) {
                self.0.set(0); //set 0 to both flags as it was before setting it to SENTINEL
                self.1.set(0);
            }
        }
        //both flags are 0 at this point
        self.borrow_flag.set(Self::SENTINEL); //lock both flags before returning guard
        self.iterate_flag.set(Self::SENTINEL);
        Guard(&self.borrow_flag,&self.iterate_flag)
    }
    /// Note that when iterating elements, the count is not updated. Its affected only after iteration is
    /// done.
    pub fn retain(&self,func: impl FnMut(TaskKey,&DynamicFuture<'future>)->bool){
        #[cfg(debug_assertions)]
        let _guard = self.guard_retain();

        //SAFETY: we check for borrow and iteration in 'guard_retain' lock all flags and create
        //guard that restores flags on drop so if 'func' panics, flags are restored.
        let slab = unsafe{ &mut *self.slab.get() };
        slab.retain(func);
    }
}