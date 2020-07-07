//! SPSC Ringbuffer.

use atomic_enum::atomic_enum;
#[cfg(feature = "serialization")]
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

#[derive(Debug)]
struct UnsafeVec<T>(UnsafeCell<Vec<T>>);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LoadErrorKind {
    Empty,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StoreErrorKind {
    Full,
}

#[atomic_enum]
#[derive(PartialEq)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
enum LimitKind {
    Empty,
    Full,
}

#[derive(Debug)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct SpscRingbuffer<T: Copy + Default> {
    buffer: UnsafeVec<T>,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
    limit_kind: AtomicLimitKind,
    size: usize,
}

impl<T: Copy + Default> SpscRingbuffer<T> {
    pub fn new(size: usize) -> SpscRingbuffer<T> {
        SpscRingbuffer {
            buffer: UnsafeVec(UnsafeCell::new(vec![T::default(); size])),
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
        let write_index = self.write_index.load(Ordering::Acquire);
        let read_index = self.read_index.load(Ordering::Acquire);

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
        let write_index = self.write_index.load(Ordering::Acquire);
        let read_index = self.read_index.load(Ordering::Acquire);

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
        if self.is_empty() {
            return Err(LoadErrorKind::Empty);
        }

        let read_index = self.read_index.load(Ordering::Relaxed);
        let write_index = self.write_index.load(Ordering::Relaxed);

        let item = unsafe { *self.buffer.0.get().as_ref().unwrap().get_unchecked(read_index) };

        let next_read_index = (read_index + 1) % self.size;

        if next_read_index == write_index {
            self.limit_kind.store(LimitKind::Empty, Ordering::Relaxed)
        }

        self.read_index.store(next_read_index, Ordering::Release);

        Ok(item)
    }

    pub fn push(&self, item: T) -> Result<(), StoreErrorKind> {
        if self.is_full() {
            return Err(StoreErrorKind::Full);
        }

        let write_index = self.write_index.load(Ordering::Relaxed);
        let read_index = self.read_index.load(Ordering::Relaxed);

        unsafe {
            *self.buffer.0.get().as_mut().unwrap().get_unchecked_mut(write_index) = item;
        }

        let next_write_index = (write_index + 1) % self.size;

        if next_write_index == read_index {
            self.limit_kind.store(LimitKind::Full, Ordering::Relaxed)
        }

        self.write_index.store(next_write_index, Ordering::Release);

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

#[cfg(feature = "serialization")]
pub mod serialization {
    use super::*;
    use serde::{
        Deserializer,
        Serializer,
    };

    impl<T> Serialize for UnsafeVec<T>
    where T: Default + Copy + Serialize
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            let buffer = unsafe { &*self.0.get() };
            <Vec<T> as Serialize>::serialize(buffer, serializer)
        }
    }

    impl<'de, T> Deserialize<'de> for UnsafeVec<T>
    where T: Default + Copy + Deserialize<'de>
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            let buffer = <Vec<T> as Deserialize>::deserialize(deserializer)?;
            Ok(UnsafeVec(UnsafeCell::new(buffer)))
        }
    }

    impl Serialize for AtomicLimitKind {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            let kind = self.load(Ordering::Relaxed);
            <LimitKind as Serialize>::serialize(&kind, serializer)
        }
    }

    impl<'de> Deserialize<'de> for AtomicLimitKind {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            let kind = <LimitKind as Deserialize>::deserialize(deserializer)?;
            Ok(AtomicLimitKind::new(kind))
        }
    }
}
