use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;
use arrayvec::ArrayVec;

#[derive(Debug, PartialEq)]
pub enum LoadErrorKind {
    Empty,
}

#[derive(Debug, PartialEq)]
pub enum StoreErrorKind {
    Full,
}

pub struct SpscRingbuffer<T: Copy + Default> {
    buffer: UnsafeCell<ArrayVec<[T; 128]>>,
    head: AtomicUsize,
    tail: AtomicUsize,
    size: usize,
}

impl<T: Copy + Default> SpscRingbuffer<T> {
    pub fn new(size: usize) -> SpscRingbuffer<T> {
        if size >= 128 { 
            unimplemented!("SPSC Ringbuffer sizes above 127 are not supported for now - awaiting const generics support");
        }

        SpscRingbuffer {
            buffer: UnsafeCell::new(ArrayVec::from([T::default(); 128])),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            size: size + 1,
        }
    }

    fn is_empty_by_ptr(&self, head: usize, tail: usize) -> bool {
        head == tail
    }
    
    fn is_full_by_ptr(&self, head: usize, tail: usize) -> bool {
        ((head + 1) % self.size) == tail
    }

    pub fn read_available(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);

        if head > tail {
            head - tail
        } else {
            (self.size - tail) + head
        }
    }

    pub fn write_available(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);

        if head < tail {
            tail - head - 1
        } else {
            (self.size - head - 1) + tail
        }
    }

    pub fn is_empty(&self) -> bool {
        // Empty condition is when both pointers are equal.
        self.is_empty_by_ptr(self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Relaxed))
    }

    pub fn is_full(&self) -> bool {
        // Full condition is when head is one less than the tail.
        self.is_full_by_ptr(self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Relaxed))
    }

    pub fn pop(&self) -> Result<T, LoadErrorKind> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if self.is_empty_by_ptr(head, tail) {
            return Err(LoadErrorKind::Empty);
        }

        let item = unsafe {
            self.buffer.get().as_ref().unwrap()[tail]
        };

        let next_tail = (tail + 1) % self.size;
        self.tail.store(next_tail, Ordering::Release);

        Ok(item)
    }

    pub fn push(&self, item: T) -> Result<(), StoreErrorKind> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if self.is_full_by_ptr(head, tail) {
            return Err(StoreErrorKind::Full);
        }

        unsafe { 
            self.buffer.get().as_mut().unwrap()[head] = item;
        }

        let next_head = (head + 1) % self.size;
        self.head.store(next_head, Ordering::Release);

        Ok(())
    }

    pub fn clear(&self) {
        let head = self.head.load(Ordering::Relaxed);
        self.tail.store(head, Ordering::Relaxed);
    }
}

unsafe impl<T: Copy + Default> Sync for SpscRingbuffer<T> {}

/// API tests
#[cfg(test)]
mod tests_api {
    use super::*;

    #[test]
    fn new() {
        SpscRingbuffer::<u32>::new(32);
    }

    #[test]
    fn push() {
        let buffer = SpscRingbuffer::<u32>::new(32);
        buffer.push(1).unwrap();
    }

    #[test]
    fn pop() {
        let buffer = SpscRingbuffer::<u32>::new(32);
        buffer.push(1).unwrap();
        assert_eq!(buffer.pop().unwrap(), 1);
    }

    #[test]
    fn is_empty() {
        let buffer = SpscRingbuffer::<u32>::new(32);
        assert!(buffer.is_empty());
        buffer.push(1).unwrap();
        assert!(!buffer.is_empty());
    }

    #[test]
    fn is_full() {
        let buffer = SpscRingbuffer::<u32>::new(32);
        assert!(!buffer.is_full());

        for i in 0..32 {
            buffer.push(i).unwrap();
        }

        assert!(buffer.is_full());
    }

    #[test]
    fn push_full() {
        let buffer = SpscRingbuffer::<u32>::new(32);

        for i in 0..32 {
            buffer.push(i).unwrap();
        }

        assert_eq!(buffer.push(1), Err(StoreErrorKind::Full));
        assert!(!buffer.is_empty());
    }

    #[test]
    fn pop_empty() {
        let buffer = SpscRingbuffer::<u32>::new(32);

        assert_eq!(buffer.pop(), Err(LoadErrorKind::Empty));
        assert!(!buffer.is_full());
    }

    #[test]
    fn values() {
        let buffer = SpscRingbuffer::<u32>::new(32);

        for i in 0..32 {
            buffer.push(i).unwrap();
        }

        for i in 0..32 {
            assert_eq!(buffer.pop().unwrap(), i);
        }
    }

    #[test]
    fn read_available() {
        let buffer = SpscRingbuffer::<u32>::new(8);

        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();

        assert_eq!(buffer.read_available(), 5);

        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        
        assert_eq!(buffer.read_available(), 5);
        
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        
        assert_eq!(buffer.read_available(), 2);

        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        
        assert_eq!(buffer.read_available(), 4);
    }

    #[test]
    fn write_available() {
        let buffer = SpscRingbuffer::<u32>::new(8);

        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();

        assert_eq!(buffer.write_available(), 3);

        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        
        assert_eq!(buffer.write_available(), 3);
        
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        
        assert_eq!(buffer.write_available(), 6);

        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        
        assert_eq!(buffer.write_available(), 4);
    }

    #[test]
    fn clear() {
        let buffer = SpscRingbuffer::<u32>::new(8);

        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        
        assert_eq!(buffer.read_available(), 5);

        buffer.clear();

        assert!(buffer.is_empty());
    }
}
