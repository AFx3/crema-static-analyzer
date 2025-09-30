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