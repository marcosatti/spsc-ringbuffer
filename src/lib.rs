#![feature(core_intrinsics)]

//! SPSC Ringbuffer.

use atomic_enum::atomic_enum;
use std::{
    cell::UnsafeCell,
    intrinsics::unlikely,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

#[derive(Debug, PartialEq)]
pub enum LoadErrorKind {
    Empty,
}

#[derive(Debug, PartialEq)]
pub enum StoreErrorKind {
    Full,
}

#[atomic_enum]
enum LimitKind {
    Empty,
    Full,
}

pub struct SpscRingbuffer<T: Copy + Default> {
    buffer: UnsafeCell<Vec<T>>,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
    limit_kind: AtomicLimitKind,
    size: usize,
}

impl<T: Copy + Default> SpscRingbuffer<T> {
    pub fn new(size: usize) -> SpscRingbuffer<T> {
        SpscRingbuffer {
            buffer: UnsafeCell::new(vec![T::default(); size]),
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
            limit_kind: AtomicLimitKind::new(LimitKind::Empty),
            size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.read_available() == 0
    }

    pub fn is_full(&self) -> bool {
        self.write_available() == 0
    }

    pub fn clear(&self) {
        self.write_index.store(0, Ordering::Relaxed);
        self.read_index.store(0, Ordering::Relaxed);
        self.limit_kind.store(LimitKind::Empty, Ordering::Relaxed);
    }

    pub fn read_available(&self) -> usize {
        let write_index = self.write_index.load(Ordering::Relaxed);
        let read_index = self.read_index.load(Ordering::Relaxed);

        if write_index == read_index {
            match self.limit_kind.load(Ordering::Relaxed) {
                LimitKind::Empty => 0,
                LimitKind::Full => self.size,
            }
        } else if write_index > read_index {
            write_index - read_index
        } else {
            (self.size - read_index) + write_index
        }
    }

    pub fn write_available(&self) -> usize {
        let write_index = self.write_index.load(Ordering::Relaxed);
        let read_index = self.read_index.load(Ordering::Relaxed);

        if write_index == read_index {
            match self.limit_kind.load(Ordering::Relaxed) {
                LimitKind::Empty => self.size,
                LimitKind::Full => 0,
            }
        } else if write_index < read_index {
            read_index - write_index
        } else {
            (self.size - write_index) + read_index
        }
    }

    pub fn pop(&self) -> Result<T, LoadErrorKind> {
        if unlikely(self.is_empty()) {
            return Err(LoadErrorKind::Empty);
        }

        let read_index = self.read_index.load(Ordering::Relaxed);
        let write_index = self.write_index.load(Ordering::Relaxed);

        let item = unsafe { self.buffer.get().as_ref().unwrap()[read_index] };

        let next_read_index = (read_index + 1) % self.size;

        if next_read_index == write_index {
            self.limit_kind.store(LimitKind::Empty, Ordering::Relaxed)
        }

        self.read_index.store(next_read_index, Ordering::Relaxed);

        Ok(item)
    }

    pub fn push(&self, item: T) -> Result<(), StoreErrorKind> {
        if unlikely(self.is_full()) {
            return Err(StoreErrorKind::Full);
        }

        let write_index = self.write_index.load(Ordering::Relaxed);
        let read_index = self.read_index.load(Ordering::Relaxed);

        unsafe {
            self.buffer.get().as_mut().unwrap()[write_index] = item;
        }

        let next_write_index = (write_index + 1) % self.size;

        if next_write_index == read_index {
            self.limit_kind.store(LimitKind::Full, Ordering::Relaxed)
        }

        self.write_index.store(next_write_index, Ordering::Relaxed);

        Ok(())
    }
}

unsafe impl<T: Copy + Default> Sync for SpscRingbuffer<T> {
}

unsafe impl<T: Copy + Default> Send for SpscRingbuffer<T> {
}

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

        for i in 0..32 {
            buffer.push(i).unwrap();
            assert!(!buffer.is_empty());
        }
    }

    #[test]
    fn is_full() {
        let buffer = SpscRingbuffer::<u32>::new(32);

        for i in 0..32 {
            assert!(!buffer.is_full());
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

        assert_eq!(buffer.read_available(), 0);

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

        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();

        assert_eq!(buffer.read_available(), 0);
    }

    #[test]
    fn write_available() {
        let buffer = SpscRingbuffer::<u32>::new(8);

        assert_eq!(buffer.write_available(), 8);

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
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();
        buffer.push(1).unwrap();

        assert_eq!(buffer.write_available(), 0);

        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();
        buffer.pop().unwrap();

        assert_eq!(buffer.write_available(), 8);
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
