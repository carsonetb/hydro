use std::collections::HashMap;

use crate::value::Field;

type ScopeItem<'ctx> = HashMap<String, Field<'ctx>>;
pub struct Scope<'ctx>(Vec<ScopeItem<'ctx>>);

impl<'ctx> Scope<'ctx> {
    pub fn new() -> Self {
        Scope(Vec::<ScopeItem<'ctx>>::new())
    }

    pub fn add_field(&mut self, name: String, field: Field<'ctx>) {
        let current = self.current_scope_mut();
        if current.len() == 0 {
            panic!();
        }
        current.insert(name, field);
    }

    pub fn push_scope(&mut self) {
        self.0.push(ScopeItem::new());
    }

    pub fn pop_scope(&mut self) -> Option<()> {
        let scope = self.0.pop();
        match scope {
            None => None,
            Some(mut scope) => {
                for (_, field) in scope.iter_mut() {
                    field.exit_scope();
                }
                Some(())
            }
        }
    }

    pub fn current_scope(&self) -> &ScopeItem<'ctx> {
        self.0
            .last()
            .expect("Cannot get current scope because no scopes have been pushed to the stack.")
    }

    pub fn current_scope_mut(&mut self) -> &mut ScopeItem<'ctx> {
        self.0
            .last_mut()
            .expect("Cannot get current scope because no scopes have been pushed to stack.")
    }

    pub fn get_field(&self, name: String) -> Option<&Field<'ctx>> {
        for scope in self.0.iter().rev() {
            if scope.contains_key(&name.clone()) {
                return Some(scope.get(&name.clone()).unwrap());
            }
        }
        None
    }
}
