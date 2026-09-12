use crate::ast::{LabelPredicate, MayPredicate};
use crate::truth::Truth;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CellValue {
    Bottom,
    Boxtimes,
    Alloc,
    Freed,
    Mb,
    Immb,
    Mv,
    Top,
}

impl CellValue {
    /// Exact Phase-5 CellValue order:
    /// BOTTOM <= all; ALLOC <= MB/IMMB/MV; all <= TOP.
    pub fn leq(self, other: Self) -> bool {
        use CellValue::*;
        match (self, other) {
            (x, y) if x == y => true,
            (Bottom, _) => true,
            (_, Top) => true,
            (Alloc, Mb | Immb | Mv) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramLanguage {
    Rust,
    C,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramVariable {
    /// Globally unique cross-language identifier used in CQPL environments.
    pub id: String,
    pub language: ProgramLanguage,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Alloc,
    Drop,
    Read,
    Write,
    Use,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLabel {
    pub predicate: EventKind,
    pub variable: String,
}

/// One implementation-level AbstractMemory component. All variables in
/// `aliases` denote the same abstract allocation at this program point and
/// therefore share one CellValue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractCell {
    pub aliases: Vec<String>,
    pub value: CellValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractMemoryAnnotation {
    #[serde(default)]
    pub cells: Vec<AbstractCell>,
}

impl AbstractMemoryAnnotation {
    pub fn value_of(&self, var: &str) -> CellValue {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.value)
            .unwrap_or(CellValue::Bottom)
    }

    pub fn aliases_of(&self, var: &str) -> BTreeSet<String> {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.aliases.iter().cloned().collect())
            .unwrap_or_else(|| BTreeSet::from([var.to_string()]))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedNode {
    pub id: String,
    #[serde(default)]
    pub successors: Vec<String>,
    #[serde(default)]
    pub labels: Vec<EventLabel>,
    #[serde(default)]
    pub pre: AbstractMemoryAnnotation,
    #[serde(default)]
    pub post: AbstractMemoryAnnotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedIcfg {
    pub schema_version: u32,
    pub entry: String,
    /// Quantifier domain. It intentionally includes Rust and C variables.
    pub variables: Vec<ProgramVariable>,
    pub nodes: Vec<AnnotatedNode>,
}

#[derive(Debug, Clone)]
pub struct Kripke {
    pub entry: String,
    pub variables: BTreeMap<String, ProgramVariable>,
    pub nodes: BTreeMap<String, AnnotatedNode>,
}

impl Kripke {
    pub fn from_annotated_icfg(input: AnnotatedIcfg) -> Result<Self, String> {
        if input.schema_version != 1 {
            return Err(format!("unsupported annotated ICFG schema_version {}; expected 1", input.schema_version));
        }

        let mut variables = BTreeMap::new();
        for v in input.variables {
            if variables.insert(v.id.clone(), v).is_some() {
                return Err("duplicate program-variable id in annotated ICFG".into());
            }
        }
        if variables.is_empty() {
            return Err("annotated ICFG contains no program variables".into());
        }

        let mut nodes = BTreeMap::new();
        for n in input.nodes {
            validate_memory(&n.id, "pre", &n.pre, &variables)?;
            validate_memory(&n.id, "post", &n.post, &variables)?;
            if nodes.insert(n.id.clone(), n).is_some() {
                return Err("duplicate node id in annotated ICFG".into());
            }
        }
        if !nodes.contains_key(&input.entry) {
            return Err(format!("entry node '{}' is not present", input.entry));
        }

        for node in nodes.values() {
            for succ in &node.successors {
                if !nodes.contains_key(succ) {
                    return Err(format!("node '{}' references missing successor '{}'", node.id, succ));
                }
            }
            for label in &node.labels {
                if !variables.contains_key(&label.variable) {
                    return Err(format!("node '{}' label references undeclared variable '{}'", node.id, label.variable));
                }
            }
        }

        Ok(Self { entry: input.entry, variables, nodes })
    }

    pub fn variable_ids(&self) -> impl Iterator<Item = &String> { self.variables.keys() }

    /// Alias component at this concrete program point in the abstract Kripke.
    /// Labels are associated with execution of the block, so both pre and post
    /// alias evidence is conservatively visible to label matching.
    pub fn aliases_at(&self, node_id: &str, var: &str) -> BTreeSet<String> {
        let Some(node) = self.nodes.get(node_id) else {
            return BTreeSet::from([var.to_string()]);
        };
        let mut aliases = node.pre.aliases_of(var);
        aliases.extend(node.post.aliases_of(var));
        aliases.insert(var.to_string());
        aliases
    }

    /// TaintMayHold from the theory. The implementation-level alias grouping is
    /// already encoded inside Pi#_post; no extra global alias closure is added.
    pub fn may_hold(&self, node_id: &str, var: &str, p: MayPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let atom = match p {
            MayPredicate::Alloc => CellValue::Alloc,
            MayPredicate::Drop => CellValue::Freed,
            MayPredicate::OwnForg => CellValue::Mv,
        };
        if atom.leq(node.post.value_of(var)) { Truth::Unknown } else { Truth::False }
    }

    /// Exact syntactic label predicate, lifted only through the MAY-alias
    /// component available at this node (pre/post), including Rust<->C aliases.
    pub fn label_hold(&self, node_id: &str, var: &str, p: LabelPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let aliases = self.aliases_at(node_id, var);
        let holds = node.labels.iter().any(|label| {
            if !aliases.contains(&label.variable) { return false; }
            match p {
                LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
                LabelPredicate::Drop => label.predicate == EventKind::Drop,
                LabelPredicate::Read => label.predicate == EventKind::Read,
                LabelPredicate::Write => label.predicate == EventKind::Write,
                LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
            }
        });
        if holds { Truth::True } else { Truth::False }
    }
}

fn validate_memory(
    node_id: &str,
    which: &str,
    mem: &AbstractMemoryAnnotation,
    variables: &BTreeMap<String, ProgramVariable>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for cell in &mem.cells {
        if cell.aliases.is_empty() {
            return Err(format!("node '{node_id}' {which} contains an empty alias component"));
        }
        for v in &cell.aliases {
            if !variables.contains_key(v) {
                return Err(format!("node '{node_id}' {which} references undeclared variable '{v}'"));
            }
            if !seen.insert(v.clone()) {
                return Err(format!("node '{node_id}' {which}: variable '{v}' appears in more than one abstract allocation"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(aliases: &[&str], value: CellValue) -> AbstractMemoryAnnotation {
        AbstractMemoryAnnotation { cells: vec![AbstractCell {
            aliases: aliases.iter().map(|x| x.to_string()).collect(), value
        }] }
    }

    fn base() -> AnnotatedIcfg {
        AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![
                ProgramVariable { id: "rust::x".into(), language: ProgramLanguage::Rust, display: None, function: None },
                ProgramVariable { id: "c::p".into(), language: ProgramLanguage::C, display: None, function: None },
            ],
            nodes: vec![AnnotatedNode {
                id: "b0".into(), successors: vec![],
                labels: vec![EventLabel { predicate: EventKind::Drop, variable: "c::p".into() }],
                pre: mem(&["rust::x", "c::p"], CellValue::Alloc),
                post: mem(&["rust::x", "c::p"], CellValue::Top),
            }],
        }
    }

    #[test]
    fn cross_language_alias_lifts_labels_and_post_state() {
        let k = Kripke::from_annotated_icfg(base()).unwrap();
        assert_eq!(k.label_hold("b0", "rust::x", LabelPredicate::Drop), Truth::True);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Alloc), Truth::Unknown);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Drop), Truth::Unknown);
    }

    #[test]
    fn rejects_overlapping_abstract_components_at_one_program_point() {
        let mut input = base();
        input.nodes[0].post.cells.push(AbstractCell { aliases: vec!["rust::x".into()], value: CellValue::Freed });
        assert!(Kripke::from_annotated_icfg(input).is_err());
    }
}
