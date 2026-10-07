// Internal unit tests for src/runtime/storage.rs.
use super::{SharedCell, SharedVec};

#[test]
fn immutable_sequence_values_can_be_cloned() {
    let original = SharedVec::new(vec![1, 2, 3]);
    let copy = original.clone();

    assert_eq!(&*original, &*copy);
}

#[test]
fn shared_cells_preserve_identity_across_clones() {
    let original = SharedCell::new(vec![1]);
    let alias = original.clone();

    alias.borrow_mut().push(2);

    assert_eq!(&*original.borrow(), &[1, 2]);
}
