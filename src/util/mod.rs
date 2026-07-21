use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Bounded VecDeque for fixed-size history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedVecDeque<T> {
    buf: VecDeque<T>,
    cap: usize,
}

impl<T> BoundedVecDeque<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    pub fn push(&mut self, v: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.buf.iter()
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn front(&self) -> Option<&T> {
        self.buf.front()
    }

    pub fn back(&self) -> Option<&T> {
        self.buf.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_vec_deque_keeps_capacity() {
        let mut b = BoundedVecDeque::new(3);
        b.push(1);
        b.push(2);
        b.push(3);
        assert_eq!(b.len(), 3);
        b.push(4);
        assert_eq!(b.len(), 3);
        assert_eq!(*b.front().unwrap(), 2);
        assert_eq!(*b.back().unwrap(), 4);
    }

    #[test]
    fn bounded_vec_deque_empty() {
        let b: BoundedVecDeque<i32> = BoundedVecDeque::new(5);
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn bounded_vec_deque_capacity_one() {
        let mut b = BoundedVecDeque::new(1);
        b.push(10);
        assert_eq!(b.len(), 1);
        b.push(20);
        assert_eq!(b.len(), 1);
        assert_eq!(*b.front().unwrap(), 20);
    }
}
