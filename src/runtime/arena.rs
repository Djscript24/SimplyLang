//! Typed generational arena storage for runtime-owned objects.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

use super::storage::{SharedCell, WeakCell};

static NEXT_ARENA_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Handle<T> {
    arena_id: u64,
    index: usize,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Copy for Handle<T> {}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.arena_id == other.arena_id
            && self.index == other.index
            && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Handle")
            .field("arena_id", &self.arena_id)
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

pub(crate) struct Arena<T> {
    id: u64,
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
    live: usize,
}

impl<T> Arena<T> {
    pub(crate) fn new() -> Self {
        let id = NEXT_ARENA_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("runtime arena identifier space exhausted");
        Self {
            id,
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> Handle<T> {
        self.live += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return Handle {
                arena_id: self.id,
                index,
                generation: slot.generation,
                marker: PhantomData,
            };
        }

        let index = self.slots.len();
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        Handle {
            arena_id: self.id,
            index,
            generation: 0,
            marker: PhantomData,
        }
    }

    pub(crate) fn get(&self, handle: Handle<T>) -> Option<&T> {
        if handle.arena_id != self.id {
            return None;
        }
        let slot = self.slots.get(handle.index)?;
        (slot.generation == handle.generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        if handle.arena_id != self.id {
            return None;
        }
        let slot = self.slots.get_mut(handle.index)?;
        (slot.generation == handle.generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub(crate) fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        if handle.arena_id != self.id {
            return None;
        }
        let slot = self.slots.get_mut(handle.index)?;
        if slot.generation != handle.generation {
            return None;
        }

        let value = slot.value.take()?;
        self.live -= 1;
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            self.free.push(handle.index);
        }
        Some(value)
    }

    #[allow(
        dead_code,
        reason = "live slot counts will support runtime heap accounting"
    )]
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    #[allow(dead_code, reason = "useful for arena lifecycle checks")]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ArenaRef<T> {
    arena: WeakCell<Arena<T>>,
    handle: Handle<T>,
}

impl<T> Clone for ArenaRef<T> {
    fn clone(&self) -> Self {
        Self {
            arena: self.arena.clone(),
            handle: self.handle,
        }
    }
}

impl<T> std::fmt::Debug for ArenaRef<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArenaRef")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl<T: PartialEq> PartialEq for ArenaRef<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.same_instance(other) {
            return true;
        }
        if !self.arena.ptr_eq(&other.arena) {
            return false;
        }
        self.with(|left| other.with(|right| left == right).unwrap_or(false))
            .unwrap_or(false)
    }
}

impl<T> ArenaRef<T> {
    pub(crate) fn identity_key(&self) -> Option<(u64, usize, u32)> {
        self.with(|_| {
            (
                self.handle.arena_id,
                self.handle.index,
                self.handle.generation,
            )
        })
    }

    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        self.handle == other.handle && self.with(|_| ()).is_some() && other.with(|_| ()).is_some()
    }

    pub(crate) fn insert(arena: SharedCell<Arena<T>>, value: T) -> Self {
        let handle = arena.borrow_mut().insert(value);
        Self {
            arena: arena.downgrade(),
            handle,
        }
    }

    pub(crate) fn get_cloned(&self) -> Option<T>
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    pub(crate) fn with<R>(&self, read: impl FnOnce(&T) -> R) -> Option<R> {
        self.arena
            .with(|arena| arena.get(self.handle).map(read))
            .flatten()
    }

    pub(crate) fn with_mut<R>(&self, update: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.arena
            .with_mut(|arena| arena.get_mut(self.handle).map(update))
            .flatten()
    }
}

impl<T: Clone> ArenaRef<Vec<T>> {
    pub(crate) fn len(&self) -> usize {
        self.with(Vec::len).unwrap_or(0)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.with(Vec::is_empty).unwrap_or(true)
    }

    pub(crate) fn get(&self, index: usize) -> Option<T> {
        self.with(|values| values.get(index).cloned()).flatten()
    }

    pub(crate) fn first(&self) -> Option<T> {
        self.get(0)
    }

    pub(crate) fn iter(&self) -> std::vec::IntoIter<T> {
        self.get_cloned().unwrap_or_default().into_iter()
    }
}

impl<K: Clone + Ord, V: Clone> ArenaRef<std::collections::BTreeMap<K, V>> {
    pub(crate) fn len(&self) -> usize {
        self.with(std::collections::BTreeMap::len).unwrap_or(0)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.with(std::collections::BTreeMap::is_empty)
            .unwrap_or(true)
    }

    pub(crate) fn get(&self, key: &K) -> Option<V> {
        self.with(|values| values.get(key).cloned()).flatten()
    }

    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.with(|values| values.contains_key(key))
            .unwrap_or(false)
    }

    pub(crate) fn iter(&self) -> std::vec::IntoIter<(K, V)> {
        self.with(|values| {
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Vec<_>>()
                .into_iter()
        })
        .unwrap_or_default()
    }

    pub(crate) fn keys(&self) -> std::vec::IntoIter<K> {
        self.with(|values| values.keys().cloned().collect::<Vec<_>>().into_iter())
            .unwrap_or_default()
    }

    pub(crate) fn values(&self) -> std::vec::IntoIter<V> {
        self.with(|values| values.values().cloned().collect::<Vec<_>>().into_iter())
            .unwrap_or_default()
    }
}
