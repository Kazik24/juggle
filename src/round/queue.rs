use std::sync::atomic::AtomicPtr;
use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

pub struct AtomicNode<T>{
    next: AtomicPtr<AtomicNode<T>>,
    value: T,
}


impl<T> AtomicNode<T>{
    pub fn new(value: T)->Self{
        Self{
            next: AtomicPtr::new(null_mut()),
            value,
        }
    }
    pub fn new_box(value: T)->Box<Self>{Box::new(Self::new(value))}
    pub fn into_inner(self)->T{self.value}
}
impl<T> AsRef<T> for AtomicNode<T>{
    fn as_ref(&self) -> &T {&self.value}
}
impl<T> Deref for AtomicNode<T>{
    type Target = T;
    fn deref(&self) -> &Self::Target {&self.value}
}
impl<T> AsMut<T> for AtomicNode<T>{
    fn as_mut(&mut self) -> &mut T {&mut self.value}
}
impl<T> DerefMut for AtomicNode<T>{
    fn deref_mut(&mut self) -> &mut Self::Target{&mut self.value}
}


pub struct AtomicLinkedQueue<T>{
    // head: AtomicPtr<InnerNode<T>>,
    // tail: AtomicPtr<InnerNode<T>>,
    mutex: Mutex<VecDeque<Box<AtomicNode<T>>>>
}

impl<T> AtomicLinkedQueue<T>{
    pub fn new()->Self{
        Self{mutex: Mutex::new(VecDeque::new())}
    }

    pub fn pop_front_node(&self)->Option<Box<AtomicNode<T>>>{
        let mut queue = self.mutex.lock().unwrap();
        queue.pop_front()
    }
    pub fn push_back_node(&self,node: Box<AtomicNode<T>>){
        let mut queue = self.mutex.lock().unwrap();
        queue.push_back(node);
        // let node = Box::into_raw(node.inner);
        // unsafe{
        //     loop{
        //         let tail = self.tail.load(Ordering::SeqCst);
        //         (*node).next.store(tail,Ordering::SeqCst);
        //         if self.tail.compare_and_swap(tail,node,Ordering::SeqCst) == tail {break;}
        //     }
        // }
    }
}