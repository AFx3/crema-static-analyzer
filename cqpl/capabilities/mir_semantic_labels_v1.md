# `mir_semantic_labels_v1`

Capability schema-v2 per label strutturali MIR block-level:

```text
stmt:<name>
rvalue:<family>
term:<name>
```

Le query sono `stmt_l(name)`, `rvalue_l(name)`, `term_l(name)` e devono dichiarare:

```cqpl
requires mir_semantic_labels_v1;
```

Il significato è esclusivamente **presenza nel basic block serializzato**. Non introduce ordine intra-block e non rafforza le allocation labels MAY.

## Vocabolario terminatori v6Q

```text
goto
switch_int
unwind_resume
unwind_terminate
return
unreachable
drop
call
tail_call
assert
yield
coroutine_drop
false_edge
false_unwind
inline_asm
unhandled
```

Questo set deve restare sincronizzato con `CREMA::mir_semantics::terminator_category`. L'audit final112 ha rilevato che r1b produceva `term:unwind_terminate` mentre il parser non lo accettava; r1c corregge il drift e aggiunge un test sull'intero set.

Statement e rvalue vocabulary sono elencati in `LANGUAGE.md`. Le rvalue family restano un adapter versionato sul Debug spelling del toolchain pinned e devono essere rivalidate quando cambia rustc.
