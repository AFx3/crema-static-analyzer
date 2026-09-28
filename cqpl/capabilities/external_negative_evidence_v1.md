# external_negative_evidence_v1 (ENE1)

ENE1 stores and validates negative evidence only. Its normative semantics are
[LLVM 16.0.0](https://releases.llvm.org/16.0.0/docs/LangRef.html).
It changes no CQPL truth or assessment semantics and creates no ordinary events.

Exactly three kinds are supported: `no_free_function`, `no_free_formal`, and
`no_capture_formal`. Function nofree admits explicit IR or verified existing TLI;
formal kinds admit explicit IR only. Missing evidence remains unresolved.
There are no hand-written libc negative summaries or symbol-name heuristics.

`nofree` is not post-call liveness and does not imply `nocapture`. A nofree
function may free memory allocated during the call. Captured storage may be freed
by another thread. ENE1 does not implement stronger conjunction-based refinements.

`nocapture` applies only to the particular formal pointer copy. It is not
allocation-wide noescape. An aliased unproven formal stays unproven.
Formal nofree likewise cannot be lifted to another alias or formal.
Absence of nocapture is not capture; absence of nofree is not free.

Represented bodies suppress ENE1 bodyless summaries. Each record references a
unique declaration in the validated EFX1 artifact by module/function index and
preserves its callee-declaration origin. The current EFX1 surface does not expose
callsite nofree/nocapture attributes; those are not guessed. Positional MIR
`call_arguments` prove exact same-scope formal/actual bindings independently of
allocation identity. Unknown/projected actuals cannot carry formal evidence.

The checker rejects duplicates, unsupported source/basis/kind tuples, malformed
call bindings, invalid declaration provenance, and precise conflicting positive
FreeArg/ReallocArg evidence. Positive proof is never erased. There is no general
nocapture/returned-pointer contradiction theorem in this version.

Synthetic conformance evidence is reported separately from naturally observed
corpus evidence. It demonstrates mechanism coverage, not libc guarantees.
