//! Shared roots for runtime objects that may outlive a local scope.

use std::collections::BTreeMap;

use crate::runtime::{
    arena::{Arena, ArenaRef, Handle},
    storage::SharedCell,
    value::{EnumValue, FunctionValue, SourceText, StructInstance, Value},
};

#[derive(Clone, Default)]
pub(crate) struct RuntimeHeap {
    functions: SharedCell<Arena<FunctionValue>>,
    structs: SharedCell<Arena<StructInstance>>,
    enums: SharedCell<Arena<EnumValue>>,
    sources: SharedCell<Arena<SourceText>>,
    sequences: SharedCell<Arena<Vec<Value>>>,
    hashes: SharedCell<Arena<BTreeMap<String, Value>>>,
}

impl RuntimeHeap {
    pub(crate) fn insert_function(&self, function: FunctionValue) -> Handle<FunctionValue> {
        self.functions.borrow_mut().insert(function)
    }

    pub(crate) fn function(&self, handle: Handle<FunctionValue>) -> Option<FunctionValue> {
        self.functions.borrow().get(handle).cloned()
    }

    pub(crate) fn with_function<R>(
        &self,
        handle: Handle<FunctionValue>,
        inspect: impl FnOnce(&FunctionValue) -> R,
    ) -> Option<R> {
        self.functions.borrow().get(handle).map(inspect)
    }

    pub(crate) fn with_function_mut<R>(
        &self,
        handle: Handle<FunctionValue>,
        update: impl FnOnce(&mut FunctionValue) -> R,
    ) -> Option<R> {
        self.functions.borrow_mut().get_mut(handle).map(update)
    }

    pub(crate) fn insert_struct(&self, instance: StructInstance) -> ArenaRef<StructInstance> {
        ArenaRef::insert(self.structs.clone(), instance)
    }

    pub(crate) fn insert_enum(&self, value: EnumValue) -> ArenaRef<EnumValue> {
        ArenaRef::insert(self.enums.clone(), value)
    }

    pub(crate) fn insert_source(&self, source: SourceText) -> ArenaRef<SourceText> {
        ArenaRef::insert(self.sources.clone(), source)
    }

    pub(crate) fn insert_sequence(&self, values: Vec<Value>) -> ArenaRef<Vec<Value>> {
        ArenaRef::insert(self.sequences.clone(), values)
    }

    pub(crate) fn insert_hash(
        &self,
        values: BTreeMap<String, Value>,
    ) -> ArenaRef<BTreeMap<String, Value>> {
        ArenaRef::insert(self.hashes.clone(), values)
    }
}
