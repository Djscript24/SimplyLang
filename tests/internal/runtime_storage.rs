// Internal unit tests for src/runtime/storage.rs.
use std::collections::BTreeMap;

use super::{SharedCell, SharedMap, SharedVec};

#[test]
fn vector_mutation_detaches_shared_storage() {
    let mut original = SharedVec::new(vec![1, 2, 3]);
    let copy = original.clone();

    original.make_mut()[0] = 9;

    assert_eq!(&*original, &[9, 2, 3]);
    assert_eq!(&*copy, &[1, 2, 3]);
}

#[test]
fn map_mutation_detaches_shared_storage() {
    let mut initial = BTreeMap::new();
    initial.insert("key".to_owned(), 1);
    let mut original = SharedMap::new(initial);
    let copy = original.clone();

    original.make_mut().insert("key".to_owned(), 9);

    assert_eq!(original.get("key"), Some(&9));
    assert_eq!(copy.get("key"), Some(&1));
}

#[test]
fn shared_cells_preserve_identity_across_clones() {
    let original = SharedCell::new(vec![1]);
    let alias = original.clone();

    alias.borrow_mut().push(2);

    assert_eq!(&*original.borrow(), &[1, 2]);
}
