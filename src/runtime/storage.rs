//! Runtime-owned sharing primitives.
//! Shared/SharedCell provide runtime sharing; SharedVec backs immutable value collections.
use std::{
    cell::{Ref, RefCell, RefMut},
    ops::Deref,
    rc::{Rc, Weak},
};

#[derive(Debug, PartialEq)]
pub(crate) struct Shared<T: ?Sized>(Rc<T>);

impl<T: ?Sized> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> Shared<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
}

impl<T> Shared<T>
where
    T: Clone,
{
    pub(crate) fn into_owned(self) -> T {
        Rc::try_unwrap(self.0).unwrap_or_else(|value| (*value).clone())
    }
}

impl<T: ?Sized> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) struct SharedCell<T>(Shared<RefCell<T>>);

impl<T> Clone for SharedCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for SharedCell<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SharedCell")
            .field(&self.0.borrow())
            .finish()
    }
}

impl<T: PartialEq> PartialEq for SharedCell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.borrow().eq(&other.0.borrow())
    }
}

impl<T> SharedCell<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Shared::new(RefCell::new(value)))
    }

    pub(crate) fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }

    pub(crate) fn downgrade(&self) -> WeakCell<T> {
        WeakCell(Rc::downgrade(&self.0.0))
    }
}

pub(crate) struct WeakCell<T>(Weak<RefCell<T>>);

impl<T> Clone for WeakCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> WeakCell<T> {
    pub(crate) fn with<R>(&self, read: impl FnOnce(&T) -> R) -> Option<R> {
        let cell = self.0.upgrade()?;
        let value = cell.borrow();
        Some(read(&value))
    }

    pub(crate) fn with_mut<R>(&self, update: impl FnOnce(&mut T) -> R) -> Option<R> {
        let cell = self.0.upgrade()?;
        let mut value = cell.borrow_mut();
        Some(update(&mut value))
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl<T: Default> Default for SharedCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SharedVec<T>(Shared<Vec<T>>);

impl<T> SharedVec<T> {
    pub(crate) fn new(values: Vec<T>) -> Self {
        Self(Shared::new(values))
    }

    pub(crate) fn into_owned(self) -> Vec<T>
    where
        T: Clone,
    {
        self.0.into_owned()
    }
}

impl<T> Deref for SharedVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
