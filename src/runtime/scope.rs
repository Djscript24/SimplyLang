use std::collections::HashMap;

use super::value::Value;

pub(crate) struct ScopeStack {
    scopes: Vec<HashMap<String, Value>>,
}

impl ScopeStack {
    pub(crate) fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    pub(crate) fn define(&mut self, name: String, value: Value) {
        self.scopes
            .last_mut()
            .expect("runtime scope stack always has a global scope")
            .insert(name, value);
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    pub(crate) fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
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

    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
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
        scopes.define("value".into(), Value::Int(1));
        scopes.push();
        scopes.define("value".into(), Value::Int(2));

        assert_eq!(scopes.lookup("value"), Some(&Value::Int(2)));
        assert!(scopes.assign("value", Value::Int(3)));
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(3)));

        scopes.pop();
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(1)));
    }

    #[test]
    fn preserves_global_scope_after_excessive_pops() {
        let mut scopes = ScopeStack::new();
        scopes.define("global".into(), Value::Int(1));
        scopes.push();
        scopes.push();

        scopes.pop();
        scopes.pop();
        scopes.pop();
        scopes.pop();

        assert_eq!(scopes.lookup("global"), Some(&Value::Int(1)));
        scopes.define("still_global".into(), Value::Bool(true));
        assert_eq!(scopes.lookup("still_global"), Some(&Value::Bool(true)));
    }
}
