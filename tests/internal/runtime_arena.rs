// Internal unit tests for src/runtime/arena.rs.
use super::{Arena, ArenaRef, Handle};
use crate::runtime::storage::SharedCell;

#[test]
fn arena_handles_read_and_mutate_values() {
    let mut arena = Arena::new();
    let handle = arena.insert(String::from("first"));

    assert_eq!(arena.get(handle).map(String::as_str), Some("first"));
    arena
        .get_mut(handle)
        .expect("inserted handle resolves")
        .push_str(" value");
    assert_eq!(arena.get(handle).map(String::as_str), Some("first value"));
}

#[test]
fn removed_handles_cannot_access_reused_slots() {
    let mut arena = Arena::new();
    let stale = arena.insert(1);

    assert_eq!(arena.remove(stale), Some(1));
    let current = arena.insert(2);

    assert_eq!(stale.index, current.index);
    assert_ne!(stale, current);
    assert_eq!(arena.get(stale), None);
    assert_eq!(arena.remove(stale), None);
    assert_eq!(arena.get(current), Some(&2));
}

#[test]
fn repeated_reuse_never_revives_an_earlier_handle() {
    let mut arena = Arena::new();
    let mut stale = Vec::new();
    for value in 0..1024 {
        let handle = arena.insert(value);
        assert_eq!(arena.remove(handle), Some(value));
        stale.push(handle);
    }

    let current = arena.insert(2048);
    for handle in stale {
        assert_eq!(arena.get(handle), None);
        assert_eq!(arena.remove(handle), None);
    }
    assert_eq!(arena.get(current), Some(&2048));
}

#[test]
fn generation_overflow_retires_a_slot_instead_of_revalidating_handles() {
    let mut arena = Arena::new();
    let mut handle = arena.insert(1);
    arena.slots[handle.index].generation = u32::MAX;
    handle.generation = u32::MAX;

    assert_eq!(arena.remove(handle), Some(1));
    let current = arena.insert(2);

    assert_ne!(handle.index, current.index);
    assert_eq!(arena.get(handle), None);
    assert_eq!(arena.get(current), Some(&2));
}

#[test]
fn handles_are_typed_by_their_arena_value() {
    let mut numbers = Arena::<i64>::new();
    let number = numbers.insert(7);
    assert_eq!(numbers.get(number), Some(&7));

    let mut strings = Arena::<String>::new();
    let text: Handle<String> = strings.insert(String::from("seven"));
    assert_eq!(strings.get(text).map(String::as_str), Some("seven"));
}

#[test]
fn arena_length_tracks_live_slots() {
    let mut arena = Arena::new();
    assert!(arena.is_empty());
    let first = arena.insert(1);
    let second = arena.insert(2);
    assert_eq!(arena.len(), 2);

    arena.remove(first);
    assert_eq!(arena.len(), 1);
    arena.remove(second);
    assert!(arena.is_empty());
}

#[test]
fn handles_cannot_be_used_with_a_different_arena() {
    let mut first = Arena::new();
    let handle = first.insert(1);
    let mut second = Arena::new();
    second.insert(2);

    assert_eq!(second.get(handle), None);
    assert_eq!(second.remove(handle), None);
    assert_eq!(first.get(handle), Some(&1));
}

#[test]
fn arena_references_do_not_keep_their_heap_alive() {
    let reference = {
        let arena = SharedCell::new(Arena::new());
        ArenaRef::insert(arena, 42)
    };

    assert_eq!(reference.with(|value| *value), None);
}
