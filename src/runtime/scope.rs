//! runtime/scope.rs — runtime bindings and scopes
//! Manages nested runtime environments, binding mutability, lookup, assignment, and shadowing.
//! Key component: scope storage and binding operations.
use std::collections::HashMap;
use std::collections::HashSet;

use super::{
    arena::{Arena, Handle},
    value::Value,
};

struct BindingToken(Handle<Value>);

struct BindingArena(Arena<Value>);

impl BindingArena {
    fn new() -> Self {
        Self(Arena::new())
    }

    fn allocate(&mut self, value: Value) -> BindingToken {
        BindingToken(self.0.insert(value))
    }

    fn get(&self, binding: &BindingToken) -> &Value {
        self.0
            .get(binding.0)
            .expect("scope binding token must refer to a live arena slot")
    }

    fn get_mut(&mut self, binding: &BindingToken) -> &mut Value {
        self.0
            .get_mut(binding.0)
            .expect("scope binding token must refer to a live arena slot")
    }

    fn release(&mut self, binding: BindingToken) -> Value {
        self.0
            .remove(binding.0)
            .expect("scope binding token must be released exactly once")
    }
}

pub(crate) struct ScopeStack {
    values: BindingArena,
    scopes: Vec<HashMap<String, BindingToken>>,
    bindings: HashMap<String, Vec<usize>>,
    mutability: HashMap<String, Vec<bool>>,
    reusable_scopes: Vec<HashMap<String, BindingToken>>,
}

impl ScopeStack {
    pub(crate) fn new() -> Self {
        Self {
            values: BindingArena::new(),
            scopes: vec![HashMap::new()],
            bindings: HashMap::new(),
            mutability: HashMap::new(),
            reusable_scopes: Vec::new(),
        }
    }

    pub(crate) fn define(
        &mut self,
        name: String,
        value: Value,
        mutable: bool,
    ) -> Result<(), String> {
        let current = self
            .scopes
            .last_mut()
            .expect("runtime scope stack always has a global scope");
        if current.contains_key(&name) {
            return Err(format!(
                "variable `{name}` is already declared in this scope; declare it with `mut` to reassign it"
            ));
        }
        let binding = self.values.allocate(value);
        current.insert(name.clone(), binding);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(())
    }

    pub(crate) fn define_many(
        &mut self,
        bindings: Vec<(String, Value, bool)>,
    ) -> Result<(), String> {
        let current = self
            .scopes
            .last()
            .expect("runtime scope stack always has a global scope");
        let mut names = HashSet::with_capacity(bindings.len());
        for (name, _, _) in &bindings {
            if current.contains_key(name) || !names.insert(name.as_str()) {
                return Err(format!(
                    "variable `{name}` is already declared in this scope or destructuring target"
                ));
            }
        }

        let scope_index = self.scopes.len() - 1;
        let current = self
            .scopes
            .last_mut()
            .expect("runtime scope stack always has a global scope");
        for (name, value, mutable) in bindings {
            let binding = self.values.allocate(value);
            current.insert(name.clone(), binding);
            self.bindings
                .entry(name.clone())
                .or_default()
                .push(scope_index);
            self.mutability.entry(name).or_default().push(mutable);
        }
        Ok(())
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&Value> {
        let handle = self
            .scopes
            .last()
            .and_then(|scope| scope.get(name))
            .or_else(|| {
                let scope_index = self.bindings.get(name)?.last().copied()?;
                self.scopes.get(scope_index)?.get(name)
            })?;
        Some(self.values.get(handle))
    }

    pub(crate) fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        let current_index = self.scopes.len() - 1;
        let binding = self.scopes[current_index].get(name).or_else(|| {
            let scope_index = self.bindings.get(name)?.last().copied()?;
            self.scopes.get(scope_index)?.get(name)
        })?;
        Some(self.values.get_mut(binding))
    }

    pub(crate) fn is_mutable(&self, name: &str) -> bool {
        self.mutability
            .get(name)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn binding_scope(&self, name: &str) -> Option<usize> {
        self.bindings.get(name)?.last().copied()
    }

    pub(crate) fn current_scope_index(&self) -> usize {
        self.scopes.len() - 1
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    pub(crate) fn assign(&mut self, name: &str, value: Value) -> bool {
        if let Some(binding) = self.lookup_mut(name) {
            *binding = value;
            true
        } else {
            false
        }
    }

    pub(crate) fn assign_many(
        &mut self,
        bindings: Vec<(usize, String, Value)>,
    ) -> Result<(), String> {
        let mut names = HashSet::with_capacity(bindings.len());
        for (scope_index, name, _) in &bindings {
            let is_visible_binding = self
                .bindings
                .get(name)
                .and_then(|scopes| scopes.last())
                .is_some_and(|visible_scope| visible_scope == scope_index);
            let is_mutable = self
                .mutability
                .get(name)
                .and_then(|values| values.last())
                .copied()
                .unwrap_or(false);
            if !names.insert(name.as_str()) {
                return Err(format!(
                    "variable `{name}` appears more than once in destructuring assignment"
                ));
            }
            if !is_visible_binding || !is_mutable {
                return Err(format!(
                    "variable `{name}` is no longer a mutable assignment target"
                ));
            }
            if self
                .scopes
                .get(*scope_index)
                .and_then(|scope| scope.get(name))
                .is_none()
            {
                return Err(format!("unknown assignment target `{name}`"));
            }
        }

        for (scope_index, name, value) in bindings {
            let binding = &self.scopes[scope_index][&name];
            *self.values.get_mut(binding) = value;
        }
        Ok(())
    }

    pub(crate) fn assign_current(&mut self, name: &str, value: Value) -> bool {
        let Some(binding) = self.scopes.last().and_then(|scope| scope.get(name)) else {
            return false;
        };
        let slot = self.values.get_mut(binding);
        *slot = value;
        true
    }

    pub(crate) fn remove_current(&mut self, name: &str) -> Option<Value> {
        let binding = self
            .scopes
            .last_mut()
            .expect("runtime scope stack always has a global scope")
            .remove(name)?;
        let value = self.values.release(binding);
        if let Some(scope_indices) = self.bindings.get_mut(name) {
            scope_indices.pop();
            if scope_indices.is_empty() {
                self.bindings.remove(name);
            }
            if let Some(values) = self.mutability.get_mut(name) {
                values.pop();
                if values.is_empty() {
                    self.mutability.remove(name);
                }
            }
        }
        Some(value)
    }

    pub(crate) fn has_in_current_scope(&self, name: &str) -> bool {
        self.scopes
            .last()
            .map(|scope| scope.contains_key(name))
            .unwrap_or(false)
    }

    pub(crate) fn values_for(&self, names: &HashSet<String>) -> HashMap<String, Value> {
        let mut values = HashMap::new();
        for scope in &self.scopes {
            for (name, binding) in scope {
                if names.contains(name) {
                    values.insert(name.clone(), self.values.get(binding).clone());
                }
            }
        }
        values
    }

    pub(crate) fn push(&mut self) {
        self.scopes
            .push(self.reusable_scopes.pop().unwrap_or_default());
    }

    pub(crate) fn pop(&mut self) {
        if self.scopes.len() > 1 {
            let mut scope = self.scopes.pop().expect("scope exists after length check");
            for (name, binding) in scope.drain() {
                self.values.release(binding);
                if let Some(scope_indices) = self.bindings.get_mut(&name) {
                    scope_indices.pop();
                    if scope_indices.is_empty() {
                        self.bindings.remove(&name);
                    }
                    if let Some(values) = self.mutability.get_mut(&name) {
                        values.pop();
                        if values.is_empty() {
                            self.mutability.remove(&name);
                        }
                    }
                }
            }
            scope.clear();
            self.reusable_scopes.push(scope);
        }
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../../tests/internal/runtime_scope.rs"]
mod tests;
