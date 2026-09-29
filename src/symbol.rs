use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u128);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Stable semantic identity used by graphs and compiled references.
    pub id: SymbolId,
    /// Human-facing Unicode Text. Never authoritative after resolution.
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolTable {
    by_id: BTreeMap<SymbolId, Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolIssue {
    DuplicateId(SymbolId),
    UnknownId(SymbolId),
}

impl SymbolTable {
    pub fn insert(&mut self, symbol: Symbol) -> Result<(), SymbolIssue> {
        if self.by_id.contains_key(&symbol.id) {
            return Err(SymbolIssue::DuplicateId(symbol.id));
        }
        self.by_id.insert(symbol.id, symbol);
        Ok(())
    }

    pub fn resolve(&self, id: SymbolId) -> Result<&Symbol, SymbolIssue> {
        self.by_id
            .get(&id)
            .ok_or(SymbolIssue::UnknownId(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_is_not_symbol_identity() {
        let a = Symbol {
            id: SymbolId(1),
            display_name: "admin".into(),
        };
        let b = Symbol {
            id: SymbolId(2),
            display_name: "admin".into(),
        };

        assert_ne!(a.id, b.id);
    }

    #[test]
    fn unicode_display_names_do_not_change_resolved_identity() {
        let symbol = Symbol {
            id: SymbolId(42),
            display_name: "módulo_日本語🙂".into(),
        };

        let mut table = SymbolTable::default();
        table.insert(symbol.clone()).unwrap();

        assert_eq!(table.resolve(SymbolId(42)).unwrap(), &symbol);
    }
}
