use crate::chunk_slab::ChunkSlab;
use crate::round::algorithm::TaskKey;
use crate::round::dyn_future::DynamicFuture;
use core::cell::{UnsafeCell, Cell};
use core::ops::Deref;

pub(crate) struct Registry<'future>{
    slab: UnsafeCell<ChunkSlab<TaskKey,DynamicFuture<'future>>>,
    borrow_flag: Cell<usize>,
}

pub(crate) struct TaskRef<'a,'future>{
    inner: &'a DynamicFuture<'future>,
    borrow_flag: &'a Cell<usize>,
}

impl<'future> Deref for TaskRef<'_,'future>{
    type Target = DynamicFuture<'future>;
    fn deref(&self) -> &Self::Target { self.inner }
}

impl Drop for TaskRef<'_,'_>{
    fn drop(&mut self) {
        //decrement borrow count
        self.borrow_flag.set(self.borrow_flag.get()-1);
    }
}

impl<'future> Registry<'future>{

    pub fn new()->Self{
        Self{
            slab: UnsafeCell::new(ChunkSlab::new()),
            borrow_flag: Cell::new(0),
        }
    }
    pub fn with_capacity(cap: usize)->Self{
        Self{
            slab: UnsafeCell::new(ChunkSlab::with_capacity(cap)),
            borrow_flag: Cell::new(0),
        }
    }
    pub fn get<'a>(&'a self,key: TaskKey)->Option<TaskRef<'a,'future>>{
        //SAFETY: borrow flag guards against mutable borrows
        match unsafe{ &*self.slab.get() }.get(key) {
            None => None, //no borrow needed
            Some(task) => {
                self.borrow_flag.set(self.borrow_flag.get() + 1); //indicate borrow
                Some(TaskRef{
                    inner: task,
                    borrow_flag: &self.borrow_flag
                })
            }
        }
    }
    pub fn insert(&self,val: DynamicFuture<'future>)->TaskKey{
        unsafe{
            //SAFETY: We know that chunk slab wont reallocate or change memory of already
            //borrowed tasks when inserting
            let r = &mut *self.slab.get();
            r.insert(val)
        }
    }
    pub fn remove(&self,key: TaskKey)->Option<DynamicFuture<'future>>{
        if self.borrow_flag.get() != 0 {
            panic!("Registry: Cannot remove task that might be borrowed.");
        }
        //SAFETY: we just checked if anything is borrowed
        unsafe{ (&mut *self.slab.get()).remove(key) }
    }

    pub fn iter(&self) -> impl Iterator<Item=(usize, &DynamicFuture<'future>)>{
        struct It<'a,T>(T,&'a Cell<usize>);
        impl<T> Drop for It<'_,T>{
            fn drop(&mut self) { self.1.set(self.1.get()-1); }
        }
        impl<'a,T: Iterator> Iterator for It<'a,T>{
            type Item = T::Item;
            fn next(&mut self) -> Option<Self::Item> { self.0.next() }
        }

        self.borrow_flag.set(self.borrow_flag.get() + 1);//we will return borrow from this function

        //SAFETY we just inserted borrow flag into iterator, and it will decrement it on drop
        //so this reference is guarded
        let r = unsafe{ &*self.slab.get() };
        It(r.iter(),&self.borrow_flag)
    }
}