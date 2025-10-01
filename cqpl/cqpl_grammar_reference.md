# CQPL Grammar Reference

This document explains the CQPL grammar in detail, rule by rule, with practical examples. CQPL is a DSL designed for defining rules on program memory, taint analysis, and structural patterns.

---

## Lexical Rules

### Whitespace and Comments
```pest
WHITESPACE = _{ " " | "\t" }
NEWLINE    = _{ "\r\n" | "\n" }
COMMENT    = _{ "//" ~ (!NEWLINE ~ ANY)* }
```

- `WHITESPACE` and `NEWLINE` are ignored between tokens where `_` is used.
- `COMMENT` starts with `//` and runs until the end of the line.

Example:
```cqpl
// This is a comment
v: int|*|x   // declare variable x of type int
```

---

## File and Rules

### File Structure
```pest
file = { SOI ~ (rule_item ~ ("---" ~ NEWLINE)*)* ~ EOI }
```

- A file is a sequence of **rule items**, separated optionally by `---`.

Example file with two rules:
```cqpl
# mem_leak
## domain: memory
v: *|*|*
taint_src: \forall alloc
taint_snk: !drop

---
# use_after_free
## domain: memory
v: *|*|var_x
taint_src: \forall alloc
taint_snk: drop
|>
taint_snk: use
```

---

### Rule Item
```pest
rule_item = { rule_header ~ (NEWLINE* ~ rule_body_line)* ~ NEWLINE* }
rule_header = { "#" ~ (ASCII_ALPHANUMERIC | "_" )+ ~ (" " ~ (!NEWLINE ~ ANY)+ )? }
```

- `rule_item` starts with a **rule header**.
- Rule headers begin with `#` followed by a name.

Examples:
```cqpl
# mem_leak
# uaf
# my_custom_rule with description
```

---

### Rule Body Lines
```pest
rule_body_line = _{ domain_decl | var_decl | taint_src_decl | taint_snk_decl | blank }
blank = _{ NEWLINE }
```

The body of a rule may contain:
- **domain declarations**
- **variable declarations**
- **taint source/sink declarations**

---

## Domain Declaration
```pest
domain_decl = { "##" ~ "domain:" ~ (!NEWLINE ~ ANY)* ~ NEWLINE? }
```

Specifies the analysis domain for the rule.

Examples:
```cqpl
## domain: memory
## domain: general
```

---

## Variable Declaration
```pest
var_decl = { "v:" ~ WHITESPACE* ~ var_pattern ~ NEWLINE? }
var_pattern = { (ASCII_ALPHANUMERIC | "_" | "*" | "|" )+ }
```

Variables follow the form:
```
v: type | qualifier | name
```

- `type`: `int`, `vec`, `tuple`, `struct`, `array`, `*`, etc.
- `qualifier`: `imm`, `mut`, `*`
- `name`: specific identifier or `*`

Examples:
```cqpl
v: vec|*|*        // any Vec
v: struct|mut|x   // mutable struct named x
v: *|*|var_x      // any type, any qualifier, name var_x
```

---

## Taint Declarations

### Source
```pest
taint_src_decl = { "taint_src:" ~ statement_line ~ (NEWLINE* ~ "|>" ~ NEWLINE* ~ "taint_src:" ~ statement_line)* ~ NEWLINE? }
```

### Sink
```pest
taint_snk_decl = { ("taint_snk:" | "@taint_snk:") ~ statement_line ~ (NEWLINE ~ "|>" ~ NEWLINE ~ ("taint_snk:" | "@taint_snk:") ~ statement_line )* ~ NEWLINE? }
```

- `taint_src:` and `taint_snk:` introduce **statements**.
- Multiple statements can be sequenced using `|>`.

Example:
```cqpl
taint_src: \forall alloc
taint_snk: drop
|>
taint_snk: use
```

---

## Logic and Expressions

### Logic
```pest
logic_expr  = { order_expr }
order_expr  = { sequence_expr ~ ("|>" ~ sequence_expr)* }
sequence_expr = { or_expr }
or_expr     = { and_expr ~ ("||" ~ and_expr )* }
and_expr    = { not_expr ~ ("&&" ~ not_expr )* }
```

Operators (in order of precedence):
1. `!` – negation
2. `&&` – conjunction
3. `||` – disjunction
4. `|>` – sequence

---

### Negation
```pest
not_expr = { "!"* ~ primary }
```

Examples:
```cqpl
!drop
!!use   // double negation
```

---

### Primary Expressions
```pest
primary  = { predicate | wildcard | paren_expr | quant_expr }
paren_expr = { "(" ~ logic_expr ~ ")" }
```

Parentheses group expressions:
```cqpl
!(read && write)
```

---

### Wildcard
```pest
wildcard = { "*" }
```

Represents a "match anything" statement:
```cqpl
taint_src: *
```

---

## Quantifiers
```pest
quant_expr = { ("\\forall" | "\\exists") ~ (ident | predicate) ~ ("in" ~ type_access ~ ("!=" ~ field_access)? ~ ("&&" ~ predicate)?)? }
```

- `\forall` – universal quantifier
- `\exists` – existential quantifier

Examples:
```cqpl
\forall alloc
\exists drop
\exists x in Vec.fields && use
```

---

## Predicates
```pest
predicate = { ident ~ ("(" ~ arg_list? ~ ")" )? }
arg_list  = { arg ~ ("," ~ arg)* }
arg       = { field_access | ident }
```

Predicates express program events.

Examples:
```cqpl
alloc
drop
read(x)
assign(x, y)
```

---

## Field Access
```pest
type_access  = { ident ~ "." ~ "fields" }
field_access = { ident ~ "." ~ ident }
```

Examples:
```cqpl
Vec.fields       // all fields of Vec
x.field          // access to field "field" of variable x
```

---

## Ordered Statements
```pest
ordered_statements = { statement_line ~ (NEWLINE* ~ "|>" ~ NEWLINE* ~ statement_line)* }
statement_line = { logic_expr }
```

Allows explicit ordering of statements using `|>`.

Example:
```cqpl
taint_snk: drop
|>
taint_snk: use
```

---

# Full Examples

### 1. Detect memory leak
```cqpl
# mem_leak
## domain: memory
v: *|*|*

taint_src: \forall alloc
taint_snk: !drop
```

### 2. Use-after-free (UAF)
```cqpl
# uaf
## domain: memory
v: *|*|var_x

taint_src: \forall alloc
taint_snk: drop
|>
taint_snk: use
```

### 3. Structural property
```cqpl
# test_and
## domain: general
v: vec|*|*

taint_src: \forall alloc
taint_snk: \exists x in Vec.fields && drop
```

---

# BNF-style Cheat Sheet

```bnf
file           ::= rule_item ("---" rule_item)*
rule_item      ::= "#" IDENT TEXT? NEWLINE (rule_body_line NEWLINE)*
rule_body_line ::= domain_decl | var_decl | taint_src_decl | taint_snk_decl

domain_decl    ::= "## domain:" TEXT
var_decl       ::= "v:" var_pattern
var_pattern    ::= IDENT ("|" IDENT)*

taint_src_decl ::= "taint_src:" logic_expr ("|>" "taint_src:" logic_expr)*
taint_snk_decl ::= ("taint_snk:" | "@taint_snk:") logic_expr ("|>" ("taint_snk:" | "@taint_snk:") logic_expr)*

logic_expr     ::= or_expr ("|>" or_expr)*
or_expr        ::= and_expr ("||" and_expr)*
and_expr       ::= not_expr ("&&" not_expr)*
not_expr       ::= "!"* primary
primary        ::= predicate | quant_expr | wildcard | "(" logic_expr ")"

quant_expr     ::= ("\forall" | "\exists") (ident | predicate) ("in" type_access ("!=" field_access)? ("&&" predicate)?)?
predicate      ::= ident ("(" arg_list? ")")?
arg_list       ::= arg ("," arg)*
arg            ::= ident | field_access

wildcard       ::= "*"
type_access    ::= ident ".fields"
field_access   ::= ident "." ident
ident          ::= [A-Za-z0-9_]+
```



# CQPL Parser — Detailed Documentation

> This document documents the CQPL parser implementation (Rust + Pest), the grammar used to parse CQPL rules, the AST types, and the key functions that build and evaluate the AST.

---

# 1. Overview

This crate implements a parser for a small query language (CQPL) expressed with a Pest grammar. The parser produces an AST (Rust structs/enums) representing rules that have a header, an optional domain, variable declarations, and taint blocks (`taint_src`, `taint_snk`). The AST can be used for further analyses (pattern matching, semantic checks, or later abstract interpretation).

Key pieces:

- `CQPLParser` — Pest parser (driven by `cqpl.pest`).
- AST types: `RuleDef`, `TaintBlock`, `Statement`, `Predicate`, `Variable`, `Type`, etc.
- Builder functions: `build_ast`, `parse_ordered_statements`, `parse_statement`, `parse_predicate`.
- Utility & semantic helpers: `validate_rule`, `semantically_eq`, `Statement::eval_with_env`.

This doc explains how grammar constructs map to AST nodes, what each function does, invariants and notable implementation details.

---

# 2. Grammar summary (PEG rules)

Relevant excerpt (conceptual summary):

- `file` — whole input: sequence of zero or more `rule_item`.
- `rule_item` — a rule header `# name ...` plus `rule_body_line` entries: domain, variables, `taint_src` and `taint_snk` declarations.
- `domain_decl` — `## domain: <text>`; used to set `Domain::Memory` when matched.
- `var_decl` — `v: TYPE|QUAL|NAME` where each slot is optional (the code splits on `|`).
- `taint_src_decl` and `taint_snk_decl` — each is a sequence of `statement_line` blocks; blocks can be chained with `|>` token.
- `logic_expr`, `order_expr`, `sequence_expr`, `or_expr`, `and_expr`, `not_expr`, `primary` — these implement boolean logic and composition operators. `|>` appears at the `order_expr`/`sequence_expr` level and maps to `Statement::Then`.
- `quant_expr` supports three forms:
  - `\forall <bound_var> . <logic_expr>` (full form)
  - `\forall <bound_var> in <type_access> (!= <field>)? (&& <predicate>)?` (in fields)
  - `\forall <bound_var>` (bare bound variable — fallback)

- `predicate` — shape `ident ( arg_list? )?`. Arguments can be identifiers (variables) or `field_access` (`x.y`).

(See the `cqpl.pest` grammar for the exact syntax provided earlier.)

---

# 3. AST types and their roles

The core AST types are implemented as Rust `struct`s and `enum`s. Below each is described and the intended semantics are explained.

## 3.1 `RuleDef`

```rust
pub struct RuleDef {
    pub name: Vec<String>,          // rule header tokens (human friendly name)
    pub domain: Domain,             // Memory or General
    pub variables: Vec<Variable>,   // declared variables in rule body
    pub taint_src: Vec<TaintBlock>, // ordered taint source blocks
    pub taint_snk: Vec<TaintBlock>, // ordered taint sink blocks
}
```

A `RuleDef` is one top-level rule parsed from the input. `taint_src` and `taint_snk` are sequences of `TaintBlock` representing the blocks (possibly chained with `|>`).

## 3.2 `BlockKind` and `TaintBlock`

```rust
pub enum BlockKind { Src, Snk }

pub struct TaintBlock {
    pub kind: BlockKind,              // whether this block belongs to sources or sinks
    pub statements: Vec<Statement>,   // statements contained in the block
    pub next_op: Option<SequenceOp>,  // Some(Then) if followed by `|>` in the input
}
```

`TaintBlock` is the unit inside `taint_src` or `taint_snk`. `next_op` captures whether the user wrote `|>` after this block, connecting it to the next.

`SequenceOp::Then` corresponds to the `|>` operator.

## 3.3 Variables and types

```rust
pub struct Variable {
    pub v_type: Option<Type>,
    pub qualifier: Option<Qualifier>,
    pub name: Option<VarName>,
}

pub enum VarName { Named(String), Any }
```

Variables are declared in rule headers `v: ...`. The parser splits a pattern on `|` and maps slots appropriately. `VarName::Any` is used for wildcard `*`.

Supported `Type` variants include `Vec`, `Struct`, `Union`, `Any`, etc. The `Qualifier` models `imm`, `mut`, or `*`.

## 3.4 `Statement` and `Predicate`

`Statement` is the core boolean/formula AST used inside `TaintBlock`:

```rust
pub enum Statement {
    Predicate(Predicate),
    And(Box<Statement>, Box<Statement>),
    Or(Box<Statement>, Box<Statement>),
    Not(Box<Statement>),
    Then(Box<Statement>, Box<Statement>), // corresponds to `|>`
    Wildcard,
    Quantified { quant: Quantifier, var: Option<VarName>, cond: Box<Statement> },
}
```

`Predicate` enumerates the language primitives:

```rust
pub enum Predicate {
    Alloc(Option<Term>),
    Drop(Option<Term>),
    Use(Option<Term>),
    Read(Option<Term>),
    Write(Option<Term>),
    Assign(Option<Term>),
    Allocator(Language, AllocatorType),
    InFields(Type),
    Custom(String, Vec<Term>),
    OwnForg(Option<Term>),
    OwnBack(Option<Term>),
}
```

`Term` models predicate arguments:

```rust
pub enum Term { Var(String), FieldAccess { base: String, field: String }, Literal(String) }
```

`Predicate::Allocator(lang, alloc_type)` requires exactly two arguments (validated at parse time). `InFields(Type)` is produced when parsing `<Type>.fields` in quantifiers.

## 3.5 `Quantifier` and `StmtKind`

`Quantifier` is `ForAll` or `Exists`.

`StmtKind` (`Src`, `Snk`, `Other`) is a helper enum used to reason about whether a statement belongs to a source or sink block. Note: a predicate is not intrinsically `Src` or `Snk` in the AST — that classification is driven by which declaration (`taint_src` vs `taint_snk`) the parser places the predicate in. This prevents a hard-coded mapping from predicate name -> kind and keeps the grammar flexible.

---

# 4. Parsing pipeline (functions)

The AST builder and parser helpers convert `Pairs<Rule>` produced by Pest into `Vec<RuleDef>` and inner AST nodes.

The main high-level functions are:

- `build_ast(pairs: Pairs<Rule>) -> Vec<RuleDef>`
- `parse_ordered_statements(pair: Pair<Rule>) -> Vec<Statement>`
- `parse_statement(pair: Pair<Rule>) -> Statement` (recursively builds `Statement` AST)
- `parse_predicate(pair: Pair<Rule>) -> Predicate`
- `validate_rule(rule: &RuleDef)`

Below each function is explained in detail.

## 4.1 `build_ast`

Purpose: iterate over the top-level Pest parse pairs, extract `rule_item` entries and convert them into `RuleDef` instances.

Key behavior:

1. It iterates over top-level pairs and recursively flattens `file` nodes.
2. For each `rule_item`, it initializes containers: `name`, `domain`, `variables`, `taint_src`, `taint_snk`.
3. For `rule_header`, it pushes header text into `name`.
4. For `var_decl`, it extracts the pattern after the `:` and splits by `|` into `v_type`, `qualifier`, and `var_name`, mapping textual tokens to `Type`, `Qualifier`, and `VarName`.
5. For `domain_decl` it checks whether the string contains `memory` (case-insensitive) and sets `Domain::Memory` accordingly.
6. For `taint_src_decl` and `taint_snk_decl` it:
   - iterates the inner blocks (`statement_line`) and for each block calls `parse_ordered_statements` to get a `Vec<Statement>`.
   - determines `next_op` by peeking the pairs iterator: if more blocks follow, `next_op` is `Some(SequenceOp::Then)`, otherwise `None`.
   - pushes a `TaintBlock` using the appropriate `BlockKind` (`Src` for `taint_src`, `Snk` for `taint_snk`).
7. After finishing a rule, it constructs `RuleDef` and calls `validate_rule(&rule)` (to enforce `|>` constraints) and pushes it to `rules`.

Important invariants: `TaintBlock.kind` is set to `Src` for `taint_src` declarations and `Snk` for `taint_snk` declarations. `next_op` indicates whether a `|>` chain continues.

## 4.2 `parse_ordered_statements`

A tiny helper that maps a `statement_line` pair into a `Vec<Statement>` by mapping each inner pair to `parse_statement`.

```rust
pub fn parse_ordered_statements(pair: Pair<Rule>) -> Vec<Statement> {
    pair.into_inner().map(|stmt| parse_statement(stmt)).collect()
}
```

Each `statement_line` corresponds to a `logic_expr`.

## 4.3 `parse_predicate`

Purpose: convert a `Pair<Rule>` with rule `predicate` into a `Predicate` enum.

Algorithm and details:

1. Read the pair as a trimmed string, `txt`.
2. If `txt` contains `(`, the substring before `(` is predicate name; the contents inside parentheses are split on commas to build an `args` vector of `Term`s. `arg` can be `ident` (becomes `Term::Var`) or `field_access` (becomes `Term::FieldAccess { base, field }`).
3. If there is no `(` it is a bare predicate (e.g., `alloc`).
4. Map the predicate name (lowercased) into one of the `Predicate` enum variants. If the predicate is `allocator`, check that exactly two arguments exist; map languages and allocator types, otherwise panic with a descriptive message.
5. If an unknown predicate name is encountered, the function panics: `Unknown predicate: <name>`.

Notes:

- Empty parentheses `alloc()` produce `Predicate::Alloc(None)` — the parser encodes no explicit `Term` argument.
- If the predicate expects a specific number of args (like `allocator`), `parse_predicate` enforces it at parse time.

## 4.4 `parse_statement`

This is the core recursive function that converts `Pair<Rule>` nodes representing logical expressions into `Statement` AST nodes. The function closely mirrors the grammar structure.

Important mapping:

- `sequence_expr` → fold into `Statement::Then` (each `sequence_expr` node is a chain; fold builds nested `Then` values)
- `order_expr` → same as `sequence_expr`: `|>` operator is mapped to `Then`
- `primary` → unwrap to its single child and parse recursively
- `predicate` → `Statement::Predicate(parse_predicate(pair))`
- `wildcard` → `Statement::Wildcard`
- `not_expr` → count leading `!` characters and wrap with `Statement::Not` repeatedly
- `and_expr` → fold into `Statement::And` (left-associative)
- `or_expr` → fold into `Statement::Or` (left-associative)
- `paren_expr` | `logic_expr` → parse inner expression

### Quantifier handling (detailed)

`Rule::quant_expr` is handled with special care because the grammar supports multiple forms:

1. The function first computes `quantifier` by checking whether the full token starts with `\forall` or `\exists`.
2. It consumes the first inner token (always present in the grammar): a `bound_var` (an identifier). This is stored in `bound_name`.
3. If there is no token after `bound_var`, there are two sub-cases:
   - If `bound_name` matches a predicate keyword (e.g. `alloc`, `drop`, `read` etc.) then this is the special `"no-var"` form: `\forall alloc`. In this case the parser returns `Statement::Quantified { quant, var: Some(VarName::Any), cond: Predicate(Alloc(None)) }`.
   - Otherwise if the bound is just a variable name with *no* body, it returns `Quantified { var: Named(bound_name), cond: Wildcard }`.
4. If there are tokens after the bound var, the parser looks at the next token:
   - `type_access` (like `Vec.fields`): produce `Predicate::InFields(Type::Vec)` and then optionally parse an additional predicate following it. The combined condition is the conjunction (`And`) of the `InFields` and the optional predicate.
   - `predicate` (e.g. `drop` or `alloc(x)`): convert the predicate and set it as the quantifier body.
   - anything else: parse the `next` token as a `Statement` and use it as the quantifier's condition (this allows `\forall x. (cond)` or `\forall x. read(x) && write(x)` etc.).

**Edge-cases & Panics**

- `\forall alloc` (no variable) is treated as `var = Any` and `cond = Predicate(Alloc(None))`.
- `allocator` without args panics.
- A bad `type_access` suffix (not `fields`) panics.

## 4.5 `validate_rule`

Purpose: ensure that `|>` is used only between blocks of the same kind. In practice, `|>` chains inside `taint_src` should connect only `TaintBlock` instances with `BlockKind::Src` and likewise for `taint_snk` with `BlockKind::Snk`.

Algorithm: iterate over each `taint_src` and `taint_snk` block vector; when a block has `next_op == Some(SequenceOp::Then)`, check the next block exists and that `block.kind == next_block.kind`. If not, panic with a descriptive message. This check prevents `src |> snk` or `snk |> src` from silently being accepted.

---

# 5. Evaluation and equivalence checking

Two functions provide a simple first-order logic evaluation model over a finite domain of values:

- `Statement::eval_with_env(&self, f, env, variables, possible_values) -> bool` — evaluate a statement in a given starting environment `env` using an externally provided predicate evaluator `f`.
- `semantically_eq(stmt1, stmt2, _preds, variables, possible_values, eval_fn)` — test whether two statements are semantically equal by exhaustively enumerating assignments to declared variables and comparing the evaluation results under a fixed `eval_fn`.

Both are intended as **small model-checker style helpers** that evaluate logical formulas over a finite domain of concrete values.

## 5.1 `eval_with_env` semantics (detailed)

**Signature**:

```rust
pub fn eval_with_env<F>(
    &self,
    f: &F,                  // external predicate evaluator: Fn(&Predicate, &Env) -> bool
    env: &Env,              // initial environment: mapping from var name -> concrete string value
    variables: &[Variable], // declared variables from the rule (passed but not used directly inside this function)
    possible_values: &[String], // values used by quantifiers
) -> bool
where F: Fn(&Predicate, &Env) -> bool;
```

**Behavior highlights**:

- The function clones `env` into a mutable `env_clone` to avoid side effects and then calls an internal `rec` function that performs recursive evaluation.

- `Predicate(p)` — delegated to the external function `f(p, env_mut)`. That user-supplied function is what maps a predicate and the current variable bindings into a boolean.

- `Not`, `And`, `Or` — standard boolean semantics over recursive evaluation.

- `Then(lhs, rhs)` (`|>`): **the implementation treats `Then` as a sequential implication with a strict rule**: If `lhs` evaluates to `true` under the current environment then `rhs` must also evaluate to `true` (so the `Then` returns `true` only when `lhs` is `true` and `rhs` is `true`). If `lhs` evaluates to `false`, the function returns `false` for the whole `Then`. This may differ from typical implication semantics `A -> B` (which would be `!A || B`) — it is an intentional design choice to model ordered taint propagation.

- `Wildcard` always returns `true`.

- Quantifiers:
  - `ForAll`: iterates the `possible_values` collection. For each value `val`, it chooses a key name: if the quantifier used a named var (`VarName::Named(name)`), the key is `name`; otherwise a fresh key `__quant_i` is generated. The function inserts (`env_mut.insert`) the chosen binding, evaluates `cond` under the modified environment, and then restores the previous binding (backtracking in-place). The quantifier returns `true` iff the condition holds for every possible value.
  - `Exists`: same pattern but returns `true` if `cond` holds for at least one possible value.

**Important notes**:

- Backtracking: the code inserts the new binding but preserves any previous binding by storing it in `old` and then restoring it after evaluation.

- Fresh keys for anonymous quantifications make it possible to evaluate quantified statements without overwriting program-level named bindings.

- The semantics of `Then` and the decision to return `false` when `lhs` is `false` are intentional (they model sequence checks rather than classical logical implication). This affects tests and analyses.

## 5.2 `semantically_eq` semantics (detailed)

`semantically_eq` implements a brute-force enumerator over the declared variables to test whether two statements are equivalent when interpreted by `eval_fn` across all environments derived from `possible_values`.

Algorithm summary:

1. `helper` is a recursive enumerator that takes a `remaining_vars` slice and assigns to each variable every value from `possible_values`.
2. For named variables, the environment key is the variable name; for anonymous variables (`VarName::Any` or `None`), the helper generates keys `__any_i` where `i` is a monotonically-increasing index.
3. For each assignment, the helper recurses. When there are no more variables to assign, it evaluates `stmt1.eval_with_env(eval_fn, env, declared_vars, possible_values)` and `stmt2.eval_with_env(...)` and checks equality.
4. The helper uses in-place insertion and backtracking (saving the old value in `old` and restoring it after the recursive call) so it never clones the full environment at each step.

Return value: `true` if two statements produce identical `bool` outcomes for every possible assignment of declared variables.

**Implication**: `semantically_eq` depends on `eval_fn` (the predicate interpretation) and `possible_values` (the finite domain). Changing either may cause two previously equivalent statements to diverge.

---

# 6. Examples (input -> AST sketch)

- `alloc(x)` → `Statement::Predicate(Predicate::Alloc(Some(Term::Var("x"))))`

- `alloc` (bare predicate) → `Statement::Predicate(Predicate::Alloc(None))`

- `alloc(x) |> alloc(y)` → nested `Then`:

```text
Statement::Then(
  Box::new( Statement::Predicate(Predicate::Alloc(Some(Var("x")))) ),
  Box::new( Statement::Predicate(Predicate::Alloc(Some(Var("y")))) )
)
```

- `\forall x. alloc(x)` → `Statement::Quantified { quant: ForAll, var: Some(VarName::Named("x")), cond: Box::new(Predicate(Alloc(Some(Var("x"))))) }`.

- `\forall alloc` → special case → `Quantified { quant: ForAll, var: Some(VarName::Any), cond: Predicate::Alloc(None) }`.

- `\exists x in Vec.fields && drop` → `Quantified { quant: Exists, var: Named("x"), cond: And(InFields(Vec), Predicate(Drop(None))) }`.

---

# 7. Error handling and panics

The implementation uses `panic!` in a few parse-time conditions which are treated as programmer/user errors:

- Unknown predicate name → `panic!("Unknown predicate: {}")` in `parse_predicate`.
- `allocator` predicate with wrong number of arguments → panic with diagnostics.
- `type_access` not ending with `.fields` or using an unsupported type in `fields` context → `panic!`.
- `validate_rule` panics if `|>` connects blocks with mismatched `BlockKind`.
- `quant_expr` invalid shape or unexpected inner tokens will also lead to `panic!` in the parsing code.

These are deliberate choices in the current codebase: change to `Result`-based error handling if you prefer recoverable errors and better diagnostics.

---

# 8. Extending or customizing behavior

Here are common ways you may want to extend the parser or semantics:

- **Add new predicate names**: Extend the `Predicate` enum and update `parse_predicate` mapping.
- **Add typed literals**: Extend `Term::Literal` handling and the argument parser to recognize quoted literals or numbers.
- **Make `Then` classical implication**: change `Statement::Then` semantics from strict sequence semantics to `!lhs || rhs` if you want classical logic implication.
- **Return `Result` instead of `panic!` at parse time**: change `parse_predicate` and `parse_statement` to return `Result<...>`.
- **Support richer quantifier ranges**: Instead of `possible_values: &[String]`, integrate a richer domain model, or allow a domain expression in the language.
- **Switch predicate semantics to an abstract lattice**: `eval_with_env` currently uses a boolean evaluator `Fn(&Predicate, &Env) -> bool`. Replace or wrap it with an abstract interpreter returning lattice elements for static analysis.

---

# 9. Implementation notes & rationale

- The parser is intentionally *small* and conservative: many errors are surfaced as panics to make failures obvious during development.
- The AST keeps predicates and statements generic; whether a predicate is considered a `source` or `sink` depends on whether it was parsed into `taint_src` or `taint_snk` — this decouples predicate names from their role and increases flexibility when authoring rules.
- Quantifier handling uses in-place backtracking into a single cloned environment (no deep cloning of the environment for each value) — this reduces allocation overhead, but requires careful restore of previous bindings.
- `semantically_eq` is brute-force: it enumerates all assignments for declared variables. This is intentionally simple and safe for small finite domains. 

---

# 10. Where to look in the code

- `src/ast.rs` — main AST types and parsing functions `parse_statement`, `parse_predicate`, builder `build_ast`, and helpers such as `validate_rule`.
- `cqpl.pest` — grammar file used by `pest_derive` to generate `CQPLParser`.

---
