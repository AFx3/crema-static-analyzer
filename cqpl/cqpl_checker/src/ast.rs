use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayPredicate {
    Alloc,
    Drop,
    OwnForg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPredicate {
    Alloc,
    Drop,
    Read,
    Write,
    Use,
    /// Allocation-contract UB: the deallocator family at this event is not
    /// compatible with the allocator family recorded for the bound allocation.
    AllocatorMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathQuantifier {
    Exists,
    ForAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathFormula {
    State(Box<StateFormula>),
    Next(Box<StateFormula>),
    Until(Box<StateFormula>, Box<StateFormula>),
    Eventually(Box<StateFormula>),
    Globally(Box<StateFormula>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateFormula {
    May {
        predicate: MayPredicate,
        logic_var: String,
    },
    Label {
        predicate: LabelPredicate,
        logic_var: String,
    },
    Not(Box<StateFormula>),
    And(Box<StateFormula>, Box<StateFormula>),
    Or(Box<StateFormula>, Box<StateFormula>),
    Exists {
        logic_var: String,
        body: Box<StateFormula>,
    },
    ForAll {
        logic_var: String,
        body: Box<StateFormula>,
    },
    ExistsAlloc {
        logic_var: String,
        body: Box<StateFormula>,
    },
    ForAllAlloc {
        logic_var: String,
        body: Box<StateFormula>,
    },
    Path {
        quantifier: PathQuantifier,
        formula: PathFormula,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDocument {
    pub required_capabilities: BTreeSet<String>,
    pub formula: StateFormula,
}

impl QueryDocument {
    pub fn new(required_capabilities: BTreeSet<String>, formula: StateFormula) -> Self {
        Self { required_capabilities, formula }
    }
}

impl StateFormula {
    /// Free logical variables of the formula. Official CQPL queries are closed;
    /// the CLI may supply initial bindings for debugging open formulas.
    pub fn free_vars(&self) -> BTreeSet<String> {
        fn visit(f: &StateFormula, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
            match f {
                StateFormula::May { logic_var, .. } | StateFormula::Label { logic_var, .. } => {
                    if !bound.iter().rev().any(|x| x == logic_var) {
                        out.insert(logic_var.clone());
                    }
                }
                StateFormula::Not(inner) => visit(inner, bound, out),
                StateFormula::And(a, b) | StateFormula::Or(a, b) => {
                    visit(a, bound, out);
                    visit(b, bound, out);
                }
                StateFormula::Exists { logic_var, body }
                | StateFormula::ForAll { logic_var, body }
                | StateFormula::ExistsAlloc { logic_var, body }
                | StateFormula::ForAllAlloc { logic_var, body } => {
                    bound.push(logic_var.clone());
                    visit(body, bound, out);
                    bound.pop();
                }
                StateFormula::Path { formula, .. } => match formula {
                    PathFormula::State(s)
                    | PathFormula::Next(s)
                    | PathFormula::Eventually(s)
                    | PathFormula::Globally(s) => visit(s, bound, out),
                    PathFormula::Until(a, b) => {
                        visit(a, bound, out);
                        visit(b, bound, out);
                    }
                },
            }
        }

        let mut out = BTreeSet::new();
        visit(self, &mut Vec::new(), &mut out);
        out
    }
}
