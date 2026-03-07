use std::collections::HashMap;

use crate::value::ValueField;

type ScopeItem<'ctx> = HashMap<String, ValueField<'ctx>>;
pub struct Scope<'ctx>(Vec<ScopeItem<'ctx>>);

impl<'ctx> Scope<'ctx> {
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
}
