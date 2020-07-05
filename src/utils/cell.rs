use core::cell::UnsafeCell;
use core::mem::{ManuallyDrop, forget};
use core::sync::atomic::*;
use core::ptr::read;
use core::ops::DerefMut;

pub struct AtomicCell<T>{
    mark: AtomicBool,
    cell: UnsafeCell<ManuallyDrop<T>>,
}
unsafe impl<T> Send for AtomicCell<T> where T: Send + Sync{}
unsafe impl<T> Sync for AtomicCell<T> where T: Send + Sync{}

impl<T> AtomicCell<T>{

    pub fn new(value: T)->Self{
        Self{
            mark: AtomicBool::new(false),
            cell: UnsafeCell::new(ManuallyDrop::new(value)),
        }
    }

    pub fn try_swap(&self,value: T)->Result<T,T>{
        if !self.mark.load(Ordering::Acquire) {
            if self.mark.compare_and_swap(false,true,Ordering::Relaxed) { //other thread interfered
                return Err(value);
            }
            //we know for sure we are only thread writing to this location
            //swap values
            unsafe{
                let first = self.cell.get().read_volatile();
                self.cell.get().write_volatile(ManuallyDrop::new(value));
                self.mark.store(false,Ordering::Release);
                return Ok(ManuallyDrop::into_inner(first));
            }
        } else { //we are some other thread waiting for this location
            Err(value)
        }
    }

    pub fn swap(&self,mut value: T)->T{
        loop{
            match self.try_swap(value) {
                Ok(val) => return val,
                Err(val) =>{
                    value = val;
                    spin_loop_hint();
                }
            }
        }
    }

    pub fn try_apply<F,R>(&self,func: F)->Result<R,F> where F: FnOnce(&mut T)->R, T: Copy{
        if !self.mark.load(Ordering::Acquire) {
            if self.mark.compare_and_swap(false,true,Ordering::Relaxed) { //other thread interfered
                return Err(func);
            }
            //we know for sure we are only thread writing to this location
            struct UnwindGuard<'a>(&'a AtomicBool);
            impl<'a> Drop for UnwindGuard<'a>{
                fn drop(&mut self) { //perform cleanup on normal execution and if closure panics
                    self.0.store(false,Ordering::Release);
                }
            }
            //modify value
            unsafe{
                let mut first = self.cell.get().read_volatile();
                let guard = UnwindGuard(&self.mark);
                let res = func(&mut first.deref_mut());//modify local copy to ensure volatile operations
                self.cell.get().write_volatile(first);
                drop(guard);//explicit drop
                Ok(res)
            }
        } else { //we are some other thread waiting for this location
            Err(func)
        }
    }

    pub fn apply<F,R>(&self,mut func: F)->R where F: FnOnce(&mut T)->R, T: Copy{
        loop{
            match self.try_apply(func) {
                Ok(res) => return res,
                Err(f) =>{
                    func = f;
                    spin_loop_hint();
                },
            }
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self)->&mut T{
        unsafe{ &mut *self.cell.get() }
    }
    #[inline(always)]
    pub fn into_inner(self)->T{
        unsafe{
            let data = read(self.cell.get());
            forget(self);//don't run destructor
            ManuallyDrop::into_inner(data)
        }
    }
}

impl<T> Drop for AtomicCell<T>{
    fn drop(&mut self) {
        unsafe{
            ManuallyDrop::drop(&mut *self.cell.get());
        }
    }
}


#[cfg(test)]
mod test{
    use std::sync::{Arc, Barrier};
    use super::*;
    use std::thread::{JoinHandle, spawn};
    use std::pin::Pin;
    use std::collections::HashSet;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hasher, Hash};
    use std::num::*;
    use std::ops::Range;
    use std::mem::replace;

    fn test_swap_many_case<T,F>(threads: u64, per_thread: u64, mut factory: impl FnMut(u64)->T,op: F)
    where T: Send + Sync + Eq + Hash + 'static,
          F: Fn(&AtomicCell<Option<T>>,Option<T>)->Option<T> + Send + Sync + 'static{
        let swap = Arc::new(AtomicCell::new(None));
        let op = Arc::new(op);
        let thr = (0..threads).map(|t|{
            let mut v = Vec::new();
            for i in 0..per_thread {
                v.push(Some(factory(t*per_thread + i + 1)));
            }
            (v,swap.clone(),op.clone())
        }).collect::<Vec<_>>().into_iter().map(|(vec,swap,op)|spawn(move ||{
            let mut res = Vec::new();
            for val in vec {
                res.push(op(&swap,val));
            }
            res
        })).collect::<Vec<_>>();
        let mut data = thr.into_iter().map(|j|j.join().unwrap()).flatten().collect::<HashSet<_>>();
        data.insert(op(&swap,None));
        assert!(data.contains(&None));
        let res = (1..(per_thread*threads + 1)).filter(|v|!data.contains(&Some(factory(*v)))).collect::<Vec<_>>();
        assert!(res.is_empty(),"Results not empty {:#?}",&res);
    }



    fn test_swap_single_case<T,F>(threads: usize,iters: usize,repeats: usize,default: T,unique: T,op: F)
    where T: Send + Sync + Eq + Clone + 'static,
          F: Fn(&AtomicCell<T>,T)->T + Send + Sync + 'static{
        let barriers = Arc::new((Barrier::new(threads + 1),Barrier::new(threads + 1),Barrier::new(threads + 1)));
        let swap = Arc::new(AtomicCell::new(default.clone()));
        let op = Arc::new(op);
        let handles = (0..threads).map(|_|{
            let b = barriers.clone();
            let default = default.clone();
            let unique = unique.clone();
            let swap = swap.clone();
            let op = op.clone();
            spawn(move||{
                let mut it = Vec::with_capacity(iters);
                for _ in 0..iters {
                    let mut v = Vec::with_capacity(repeats+1);
                    b.0.wait();
                    b.1.wait();
                    for _ in 0..repeats {
                        v.push(op(&swap,default.clone()));
                    }
                    b.2.wait();
                    v.push(op(&swap,default.clone()));
                    it.push(v.into_iter().find(|v|v == &unique).is_some());
                }
                it
            })
        }).collect::<Vec<_>>();

        let mut defs = Vec::with_capacity(iters);
        for _ in 0..iters {
            barriers.0.wait();
            op(&swap,default.clone());
            barriers.1.wait();
            defs.push(op(&swap,unique.clone()));
            barriers.2.wait();
        }
        let results = handles.into_iter().map(|v|v.join().unwrap()).collect::<Vec<_>>();
        assert!(defs.into_iter().all(|v|v == default));
        let len = results.iter().map(|v|v.len()).min().unwrap();
        assert_eq!(len,iters);
        for i in 0..iters {
            let count = results.iter().filter(|v|v[i]).count();
            assert_eq!(count,1); //only exactly single swap resulted in other value in each iteration
        }
    }

    #[derive(Clone,Eq,PartialEq,Hash)]
    struct TestData{
        d0: [u64;32],
        d1: [u64;32],
        d2: [u64;32],
        d3: [u64;32],
    }

    //large struct to test statistical data integrity
    impl TestData{
        pub fn new(val: u64)->Self{
            let (mut d1,mut d2,mut d3) = ([0;32],[0;32],[0;32]);
            let mut h = DefaultHasher::default();
            for a in d1.iter_mut(){
                h.write_u64(val);
                *a = h.finish();
            }
            for a in d2.iter_mut(){
                h.write_u64(val);
                *a = h.finish();
            }
            for a in d3.iter_mut(){
                h.write_u64(val);
                *a = h.finish();
            }
            Self{
                d0: [val;32],
                d1,d2,d3
            }
        }
    }
    fn swap_func<T>()->impl Fn(&AtomicCell<T>,T)->T{ |s,o|s.swap(o) }
    fn apply_func<T:Copy>()->impl Fn(&AtomicCell<T>,T)->T{
        |s,o|{
            s.apply(move|val|{
                replace(val,o)
            })
        }
    }

    #[test]
    fn test_basic(){
        let swap = AtomicCell::new(1);
        assert_eq!(swap.try_swap(2),Ok(1));
        assert_eq!(swap.swap(3),2);
        assert_eq!(swap.swap(12345),3);
        swap.try_apply(|val|{
            assert_eq!(*val,12345);
            *val = 10;
        });
        assert_eq!(swap.swap(0),10);
    }
    #[test]
    fn test_swap_single(){
        test_swap_single_case(8,1000,1000,11,22,swap_func());
        test_swap_single_case(8,1000,100,TestData::new(1),TestData::new(2),swap_func());
    }
    #[test]
    fn test_apply_single(){

        test_swap_single_case(8,1000,1000,11,22,apply_func());
    }
    #[test]
    fn test_swap_many(){
        test_swap_many_case(8,10000,|v|NonZeroU32::new(v as u32).unwrap(),swap_func());
        test_swap_many_case(8,5000,|v|TestData::new(v),swap_func());
    }
    #[test]
    fn test_apply_many(){

        test_swap_many_case(8,10000,|v|NonZeroU32::new(v as u32).unwrap(),apply_func());
    }
}