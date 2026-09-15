# Catalogo delle 12 query v6Q-r1c final112

Questo documento spiega **che cosa chiede realmente ogni file** in `queries_v2/`. Le query memory/allocator sono formule su informazione MAY: `unk` significa “non refutabile con l'astrazione corrente”, non “bug concretamente provato”.

## Riepilogo freeze final112

| Query | ff | unk | tt |
|---|---:|---:|---:|
| allocator_mismatch_ub | 70 | 42 | 0 |
| allocator_mismatch_ub_v2 | 70 | 42 | 0 |
| double_free_alloc | 63 | 49 | 0 |
| double_free_alloc_state | 63 | 49 | 0 |
| leak_alloc | 7 | 105 | 0 |
| leak_alloc_state | 7 | 105 | 0 |
| mir_rvalue_presence | 101 | 0 | 11 |
| mir_statement_presence | 9 | 0 | 103 |
| mir_structural_allocator_example | 98 | 14 | 0 |
| mir_terminator_presence | 0 | 0 | 112 |
| use_after_free_alloc | 81 | 31 | 0 |
| use_after_free_alloc_state | 81 | 31 | 0 |

Le coppie event/state e allocator v1/v2 coincidono sui 112 soggetti della freeze. È un **regression fact**, non un teorema universale.

---

## 1. `double_free_alloc.cqpl`

```cqpl
exists_alloc a. EF (
  alloc_l(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U drop_l(a)]
  )
)
```

### Cosa cerca

Un'allocazione astratta `a` per cui:

1. viene osservato un evento di allocazione;
2. successivamente un evento di drop/free;
3. successivamente un secondo drop/free;
4. fra i due release non compare una nuova `alloc_l(a)`.

### Tipo di informazione

`alloc_l(a)` e `drop_l(a)` sono allocation labels MAY (`may_abstract`).

### Interpretazione

- `unk`: pattern possibile/non refutabile;
- `ff`: pattern refutato nel grafo astratto;
- `tt`: non atteso con l'attuale identity MAY.

---

## 2. `double_free_alloc_state.cqpl`

```cqpl
requires allocation_state_v1;

exists_alloc a. EF (
  alloc(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U drop_l(a)]
  )
)
```

Differisce dalla precedente soltanto nel lifecycle start: usa `alloc(a)` da `allocation_post` invece dell'evento `alloc_l(a)`.

La freeze dà lo stesso risultato della versione event-centric su 112/112 soggetti.

---

## 3. `leak_alloc.cqpl`

```cqpl
exists_alloc a. EF (alloc_l(a) && EX EG !drop_l(a))
```

### Cosa cerca

Una possibile allocazione dopo la quale esiste un cammino massimale lungo il quale non viene osservato alcun drop dell'allocation.

### Caveat importante

È una query volutamente conservativa. `EG !drop_l(a)` può restare non refutabile quando l'identità o il release sono imprecisi.

Freeze: 105/112 `unk`.

Non interpretare `unk` come “105 leak confermati”.

---

## 4. `leak_alloc_state.cqpl`

```cqpl
requires allocation_state_v1;

exists_alloc a. EF (
  alloc(a) &&
  EX EG !drop(a)
)
```

Usa lo stato MAY allocation-centric sia per l'inizio (`alloc`) sia per la persistenza di non-freed (`!drop`).

Freeze: identica a `leak_alloc` su 112/112 soggetti.

---

## 5. `use_after_free_alloc.cqpl`

```cqpl
exists_alloc a. EF (
  alloc_l(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U use_l(a)]
  )
)
```

### Cosa cerca

Dopo un drop/free della stessa allocation, un evento `use/read/write` prima di una nuova allocazione della stessa identity.

### Limite semantico

`Box::from_raw` o altre operazioni di ricostruzione ownership non sono automaticamente `use_l` se il producer non le classifica come evento use/read/write. Questo spiega la deviazione storica `df_rand_cargo_c_ffi`: il vecchio detector aveva una classe UAF più ampia della formula CQPL corrente.

---

## 6. `use_after_free_alloc_state.cqpl`

Come la precedente, ma il lifecycle start usa:

```cqpl
alloc(a)
```

richiedendo `allocation_state_v1`.

Freeze: identica alla versione event-centric su 112/112 soggetti.

---

## 7. `allocator_mismatch_ub.cqpl`

```cqpl
requires allocation_contracts_v1;

exists_alloc a. EF (
  alloc_l(a) &&
  EX EF allocator_mismatch_l(a)
)
```

### Cosa cerca

Una possibile allocation seguita da un evento di deallocazione la cui famiglia non è dimostrata compatibile con quella dell'allocator.

### Famiglie

```text
rust_global
c_malloc
unknown
```

`unknown` è trattato come mismatch possibile, quindi produce witness MAY.

Questa è **AllocatorMismatch-UB**, non una query generica per ogni forma di undefined behavior.

---

## 8. `allocator_mismatch_ub_v2.cqpl`

```cqpl
requires allocation_contracts_v2;
```

La formula temporale è uguale alla v1, ma il producer deve fornire i deallocator contract v2 con proof basis strutturale chiusa.

Proof basis correnti sono documentate in `capabilities/allocation_contracts_v2.md`.

Nella freeze v1/v2 coincidono su 112/112 soggetti. Il valore del v2 è la maggiore qualità/provenance del contract, non una formula diversa.

---

## 9. `mir_statement_presence.cqpl`

```cqpl
requires mir_semantic_labels_v1;
EF stmt_l(assign)
```

### Significato

Verifica se esiste un nodo raggiungibile dall'entry che contiene almeno uno statement MIR categorizzato `assign`.

È una query di **coverage/struttura**, non di vulnerabilità.

- `tt`: almeno un `stmt:assign` raggiungibile;
- `ff`: nessuno.

Freeze: 103 `tt`, 9 `ff`.

---

## 10. `mir_rvalue_presence.cqpl`

```cqpl
requires mir_semantic_labels_v1;
EF rvalue_l(checked_binary_op)
```

Verifica presenza di rvalue MIR `CheckedBinaryOp` normalizzato come `checked_binary_op`.

È un probe di copertura MIR. Freeze: 11 `tt`, 101 `ff`.

---

## 11. `mir_terminator_presence.cqpl`

```cqpl
requires mir_semantic_labels_v1;
EF term_l(return)
```

Verifica presenza di un terminatore MIR `return` raggiungibile.

Freeze: 112/112 `tt`. Questo è un regression invariant osservato, non un requisito universale di ogni futuro artifact CQPL.

---

## 12. `mir_structural_allocator_example.cqpl`

```cqpl
requires mir_semantic_labels_v1;
requires allocation_contracts_v2;

exists_alloc a. EF (
  term_l(drop) &&
  allocator_mismatch_l(a)
)
```

Mostra la composizione fra:

- un fatto strutturale esatto (`term_l(drop)`), e
- un fatto allocator MAY (`allocator_mismatch_l(a)`).

Quindi un witness positivo resta `unk`.

Freeze: 14 `unk`, 98 `ff`.

## Come leggere una matrice di risultati

Per le query memory/allocator:

```text
ff  = pattern refutato nell'astrazione
unk = pattern possibile/non refutabile
```

Non trasformare automaticamente `unk` in vulnerability report concreto.

Per le query strutturali:

```text
tt = categoria presente su un nodo che soddisfa la formula
ff = categoria assente nel relativo scope raggiungibile
```

Per investigare un risultato, usare `ANALYSIS_GUIDE.md`: preservare artifact, entry, query, output JSON e sorgente del target.
