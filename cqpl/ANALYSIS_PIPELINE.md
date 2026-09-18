# CREMA + CQPL — pipeline di analisi corrente

Questo documento è il punto di ingresso breve per capire **che cosa viene analizzato, quale evidence viene prodotta, come CQPL decide `ff|unk|tt` e come leggere `subresult` e `strength`**.

Stato scientifico di riferimento:

```text
FINAL112 R2-R1.1
subjects = 112
queries  = 12
attempts = 1344

ff  = 705
unk = 413
tt  = 226

UNKNOWN assessment:
unk_true       = 273
unk_unoriented = 140

strength:
strong_abstract_evidence = 73
observational_candidate  = 200
unresolved               = 140
```

R2-R1.2 è una normalizzazione di **provenance/explainability**. Non cambia le 12 query frozen, il truth lattice o la classificazione `subresult/strength`.

## 1. Flusso completo

```text
Rust/C source
   |
   v
CREMA
   |
   +-- rustc MIR + abstract interpretation
   |     allocation state / events / contracts / dispositions
   |
   +-- Rust -> C positional binding (Bmulti)
   |     MAY AbstractAllocId identity across FFI
   |
   +-- LLVM16 evidence
   |     explicit input IR
   |     + TLI inference on an isolated clone
   |
   +-- SVF AndersenWaveDiff
   |     solved MAY points-to sets
   |
   v
annotated_icfg_v2.json
   |
   | fail-closed schema/capability/provenance validation
   v
CQPL finite Kripke model
   |
   +-- evaluate the frozen formula
   |       result = ff | unk | tt
   |
   +-- explanation pass
           reason_frontier
           witness / refutation evidence
           supporting_findings
           assessment:
             subresult
             direction
             strength
             basis
             caveats
```

Il punto fondamentale è che **truth e explanation sono separati**. L'explanation non può trasformare un `unk` in `tt` o `ff`.

## 2. Tre livelli da non confondere

### Truth

```text
result = ff | unk | tt
```

È il risultato della query sul modello astratto.

### Orientamento dell'UNKNOWN

```text
unk_true
```

significa: la query resta `unk`, ma esiste evidence direzionale positiva per il pattern cercato.

```text
unk_unoriented
```

significa: la query resta `unk` e l'evidence disponibile non giustifica un orientamento verso true o false.

`unk_false` e `unk_mixed` sono riservati finché non esiste una pipeline refutante duale esplicita.

### Strength

```text
abstract_established
strong_abstract_evidence
observational_candidate
unresolved
```

Non è una probabilità e non è una confidence percentuale. È una classe derivata dalla qualità della proof chain astratta.

## 3. Evidence Rust/C e identity

CREMA assegna `AbstractAllocId` MAY alle allocazioni. Al boundary Rust -> C:

```text
Rust actual ProgramVarId
    -- arg_index -->
C formal ProgramVarId
    -- MAY identity -->
AbstractAllocId set
```

`ffi_argument_identity_v1` rende questa correlazione auditabile.

Un operand MIR che non è rappresentabile come `ProgramVarId` (per esempio `const 7_i32`) è **fuori dal dominio del certificato**: viene saltato. Non è un errore e non è evidence negativa.

## 4. LLVM16: explicit IR e TLI sono evidence diverse

`llvm_memory_effects_v1` conserva due snapshot:

```text
explicit_input_ir
llvm_tli_inferred
```

La TLI inference viene eseguita su un clone isolato. Il clone non viene usato per costruire SVFIR/ICFG/Andersen.

Esempi di evidence utili:

```text
nofree
nocapture
returned
memory(...)
allockind("free"|"realloc")
allocptr
alloc-family
```

Regole conservative:

- `nocapture` riguarda quella copia del pointer, non l'intera allocation;
- `nofree` non è una prova generale di post-call liveness;
- `returned` è alias evidence, non ownership transfer;
- `allockind(free)+allocptr` descrive un deallocator, ma non identifica da solo quale `AbstractAllocId` sia MUST-freed.

Riferimento fissato: LLVM 16.0.0 LangRef:
<https://releases.llvm.org/16.0.0/docs/LangRef.html>.

## 5. SVF points-to

`svf_solved_points_to_v1` esporta `AndersenWaveDiff` già risolto.

```text
semantics = may
```

Un set singleton resta MAY. Non viene mai promosso a MUST.

Nei wrapper C chiamati da Rust il formal SVF può avere un set vuoto perché il caller Rust non è un `CallBase` interno al modulo C. In quel caso la correlazione cross-language utile è il certificato Bmulti/`ffi_argument_identity_v1`, non una reinterpretazione del set SVF vuoto.

## 6. External deallocation effects

`external_deallocation_effects_v1` usa tre status:

```text
certified_absent
observed_may_deallocate
unresolved
```

La `basis` primaria resta stabile per riproducibilità storica.

Esempio:

```json
{
  "status": "observed_may_deallocate",
  "basis": "structural_c_free_v1",
  "corroborating_bases": [
    "llvm16_tli_direct_callee_allockind_deallocation_v1"
  ]
}
```

La corroborazione LLVM/TLI è **additiva**: non sostituisce il basis storico e non cambia MAY in MUST.

## 7. Come leggere un risultato

Esempio UAF:

```text
result    = unk
subresult = unk_true
direction = true
strength  = observational_candidate
```

Interpretazione:

> Nel modello astratto esiste un pattern MAY `drop -> use` coerente con UAF, ma la precisione corrente non consente di promuoverlo a `tt`.

Esempio allocator mismatch:

```text
result    = unk
subresult = unk_true
strength  = strong_abstract_evidence
```

può indicare allocator/deallocator family note e diverse, pur mantenendo `unk` perché la relazione fra eventi e allocation resta MAY.

Esempio contratto incompleto:

```text
result    = unk
subresult = unk_unoriented
strength  = unresolved
```

Un allocator family sconosciuto non è evidence positiva di mismatch: potrebbe risolversi sia uguale sia diverso.

## 8. Provenance canonica

La stessa evidence usa lo stesso token ovunque.

In particolare:

```text
producer_certified_c_string_into_raw
producer_certified_c_string_from_raw
```

sono i wire token canonici sia nel finding JSON sia in `assessment.basis`.

`pta_basis:...` compare in `assessment.basis` solo se il record FFI contiene almeno una membership `svf_may_points_to`. Un PTA set vuoto può essere registrato come contesto di analisi nel certificato, ma non viene presentato come positive supporting evidence.

## 9. Gate finale

Una release di sola provenance deve mantenere:

```text
subjects = 112
queries  = 12
attempts = 1344

ff  = 705
unk = 413
tt  = 226

unk_true       = 273
unk_unoriented = 140

strong_abstract_evidence = 73
observational_candidate  = 200
unresolved               = 140
```

e deve inoltre verificare:

```text
12 frozen query files byte-identical
0 unapproved truth delta
0 legacy CString basis token
0 PTA basis cited for an empty points-to set
20/20 frozen structural C-free records LLVM/TLI-corroborated in FINAL112
```

Solo dopo questi gate il layer provenance/explainability può essere congelato.
