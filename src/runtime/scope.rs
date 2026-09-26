//! runtime/scope.rs — runtime bindings and scopes
//! Manages nested runtime environments, binding mutability, lookup, assignment, and shadowing.
//! Key component: scope storage and binding operations.
use std::collections::HashMap;
use std::collections::HashSet;

use super::value::Value;

pub(crate) struct ScopeStack {
    scopes: Vec<HashMap<String, Value>>,
    bindings: HashMap<String, Vec<usize>>,
    mutability: HashMap<String, Vec<bool>>,
    reusable_scopes: Vec<HashMap<String, Value>>,
}

impl ScopeStack {
    pub(crate) fn new() -> Self {
        Self {
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
        current.insert(name.clone(), value);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(())
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&Value> {
        if let Some(value) = self.scopes.last().and_then(|scope| scope.get(name)) {
            return Some(value);
        }
        let scope_index = self.bindings.get(name)?.last().copied()?;
        self.scopes.get(scope_index)?.get(name)
    }

    pub(crate) fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        let current_index = self.scopes.len() - 1;
        if self.scopes[current_index].contains_key(name) {
            return self.scopes[current_index].get_mut(name);
        }
        let scope_index = self.bindings.get(name)?.last().copied()?;
        self.scopes.get_mut(scope_index)?.get_mut(name)
    }

    pub(crate) fn is_mutable(&self, name: &str) -> bool {
        self.mutability
            .get(name)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(false)
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

    pub(crate) fn assign_current(&mut self, name: &str, value: Value) -> bool {
        self.scopes
            .last_mut()
            .and_then(|scope| scope.get_mut(name))
            .map(|binding| *binding = value)
            .is_some()
    }

    pub(crate) fn remove_current(&mut self, name: &str) -> Option<Value> {
        let value = self
            .scopes
            .last_mut()
            .expect("runtime scope stack always has a global scope")
            .remove(name)?;
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
            for (name, value) in scope {
                if names.contains(name) {
                    values.insert(name.clone(), value.clone());
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
            for name in scope.keys() {
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
mod tests {
    use super::ScopeStack;
    use crate::runtime::value::Value;

    #[test]
    fn resolves_locals_before_globals_and_assigns_nearest_binding() {
        let mut scopes = ScopeStack::new();
        scopes.define("value".into(), Value::Int(1), true).unwrap();
        scopes.push();
        scopes.define("value".into(), Value::Int(2), true).unwrap();

        assert_eq!(scopes.lookup("value"), Some(&Value::Int(2)));
        assert!(scopes.assign("value", Value::Int(3)));
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(3)));

        scopes.pop();
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(1)));
    }

    #[test]
    fn preserves_global_scope_after_excessive_pops() {
        let mut scopes = ScopeStack::new();
        scopes
            .define("global".into(), Value::Int(1), false)
            .unwrap();
        scopes.push();
        scopes.push();

        scopes.pop();
        scopes.pop();
        scopes.pop();
        scopes.pop();

        assert_eq!(scopes.lookup("global"), Some(&Value::Int(1)));
        scopes
            .define("still_global".into(), Value::Bool(true), false)
            .unwrap();
        assert_eq!(scopes.lookup("still_global"), Some(&Value::Bool(true)));
    }
}
