//! Shared roots for runtime objects that may outlive a local scope.

use crate::runtime::{
    arena::{Arena, ArenaRef, Handle},
    storage::SharedCell,
    value::{EnumValue, FunctionValue, SourceText, StructInstance},
};

#[derive(Clone, Default)]
pub(crate) struct RuntimeHeap {
    functions: SharedCell<Arena<FunctionValue>>,
    structs: SharedCell<Arena<StructInstance>>,
    enums: SharedCell<Arena<EnumValue>>,
    sources: SharedCell<Arena<SourceText>>,
}

impl RuntimeHeap {
    pub(crate) fn insert_function(&self, function: FunctionValue) -> Handle<FunctionValue> {
        self.functions.borrow_mut().insert(function)
    }

    pub(crate) fn function(&self, handle: Handle<FunctionValue>) -> Option<FunctionValue> {
        self.functions.borrow().get(handle).cloned()
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
}
