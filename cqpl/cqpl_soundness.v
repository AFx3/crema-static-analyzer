(* ================================================================ *)
(* cqpl_soundness.v                                                 *)
(* Witness-preserving soundness for the "may" judgment              *)
(* ================================================================ *)


From Stdlib Require Import List Bool String.
Import ListNotations.
From Stdlib Require Classical.

Parameter Block : Type.           (*ICFG Blocks*)
Parameter Var : Type.             (*Program vars*)
Parameter StateConcrete : Type.   (*concrete memory states*)
Parameter StateAbstract : Type.   (*abstract memory representation*)

Parameter gamma : StateAbstract -> list StateConcrete. (*\gamma is the concretization function: takes 1 abstract staes and returns more concrete states*)


Parameter R_B : Block -> Block -> Prop. (* Reflexive control flow relation on blocks (abstract) *)

(* Transitive closure of R_B+  *)
Inductive R_B_plus : Block -> Block -> Prop :=
  | plus_base : forall b b', 
        R_B b b' -> 
        R_B_plus b b'
  | plus_step : forall b b' b'', 
        R_B b b' -> 
        R_B_plus b' b'' -> 
        R_B_plus b b''.

(* Evaluation of logical variables *)
Definition Valuation := Var -> nat.
Parameter update : Valuation -> Var -> nat -> Valuation.

(* Formulas of the syntax*)
Inductive Formula :=
  | FAtom   : string -> Var -> Formula
  | FNot    : Formula -> Formula
  | FAnd    : Formula -> Formula -> Formula
  | FOr     : Formula -> Formula -> Formula
  | FForall : Var -> Formula -> Formula
  | FExists : Var -> Formula -> Formula
  | FThen   : Formula -> Formula -> Formula.

(* Concrete Kripke structure (and one fixed instance) *)
Parameter K_concrete : Type.
Parameter Kc : K_concrete.

(* Semantics of atomic predicates at concrete states *)
Parameter atom_holds_concrete :
  K_concrete -> StateConcrete -> Block -> Valuation -> string -> Var -> Prop.

(* Concrete semantics*)
(* Definisco quando uno stato concreto s soddisfa una formula phi. Semantica vera del programma target
    Es: models Kc s b rho (FThen p q) significa "nello stato s, c'è un blocco b' raggiungibile da b dove p vale in b e q vale in b'" *)
Fixpoint models (Kc : K_concrete) (s : StateConcrete)
                 (b : Block) (rho : Valuation) (phi : Formula) : Prop :=
  match phi with
  | FAtom name x => atom_holds_concrete Kc s b rho name x
  | FNot p       => ~ models Kc s b rho p
  | FAnd p q     => models Kc s b rho p /\ models Kc s b rho q
  | FOr p q      => models Kc s b rho p \/ models Kc s b rho q
  | FForall x p  => forall v : nat, models Kc s b (update rho x v) p
  | FExists x p  => exists v : nat, models Kc s b (update rho x v) p
  | FThen p q    =>
      exists b', R_B_plus b b' /\ models Kc s b rho p /\ models Kc s b' rho q
  end.

(*Abstract semantics of predicates p(x)
  taint_may_holds as a parametre (relazione primitiva, non definita esplicitamente, ma assunta):*)
Parameter taint_may_holds :
  StateAbstract -> Block -> Valuation -> string -> Var -> Prop.
(*Nell'abstract domain \sigma^#, al basic vlock b e sotto la valutazione \rho, è possibile che il predicato atomico p(x) sia vero”.*)
(*
Arg                 Type                                  	                Significato
StateAbstract     	tipo astratto degli stati globali (\sigma^#)	          informazione astratta su tutte le memorie del programma
Block             	tipo dei basic blocks (b)                               bb dove valuto la formula
Valuation	          funzione Var -> nat	                                    ambiente di valutazione dei quantificatori 
string            	nome del predicato (p)	                    
Var	                variabile su cui si sta applicando il predicato (x)     es. var x del programma
Prop	              risultato	un’enunciato                                  vale o non vale nel dominio astratto
*)

(* Inductive definition of the "may" judgment
   Define the inference rules. IF  MayJudgment sigma b rho phi is derivable, then phi may holds
*)
Inductive MayJudgment : StateAbstract -> Block -> Valuation -> Formula -> Prop :=
  | MJ_Atom : forall sigma b rho name x,
      taint_may_holds sigma b rho name x ->
      MayJudgment sigma b rho (FAtom name x)
  | MJ_MayNot : forall sigma b rho phi,
      (exists s, In s (gamma sigma) /\ ~ models Kc s b rho phi) ->
      MayJudgment sigma b rho (FNot phi)
  | MJ_And : forall sigma b rho p q,
      MayJudgment sigma b rho p ->
      MayJudgment sigma b rho q ->
      MayJudgment sigma b rho (FAnd p q)
  | MJ_OrL : forall sigma b rho p q,
      MayJudgment sigma b rho p ->
      MayJudgment sigma b rho (FOr p q)
  | MJ_OrR : forall sigma b rho p q,
      MayJudgment sigma b rho q ->
      MayJudgment sigma b rho (FOr p q)
  | MJ_Forall : forall sigma b rho x p,
      (forall v : nat, MayJudgment sigma b (update rho x v) p) ->
      MayJudgment sigma b rho (FForall x p)
  | MJ_Exists : forall sigma b rho x p,
      (exists v, MayJudgment sigma b (update rho x v) p) ->
      MayJudgment sigma b rho (FExists x p)
  | MJ_Then : forall sigma b rho p q,
      MayJudgment sigma b rho p ->
      (exists b', R_B_plus b b' /\ MayJudgment sigma b' rho q) ->
      MayJudgment sigma b rho (FThen p q).

(* Witness-preserving axioms  *)
Axiom and_witness_exists :
  forall sigma b rho p q,
    MayJudgment sigma b rho p ->
    MayJudgment sigma b rho q ->
    exists s, In s (gamma sigma) /\
              models Kc s b rho p /\
              models Kc s b rho q.

(* Updated witness glue for transitive closure *)
Axiom then_witness_glue_plus :
  forall sigma b b' rho p q,
    MayJudgment sigma b rho p ->
    R_B_plus b b' ->
    MayJudgment sigma b' rho q ->
    exists s, In s (gamma sigma) /\
              models Kc s b rho p /\
              models Kc s b' rho q.

Axiom atom_taint_concrete_witness :
  forall sigma b rho name x,
    taint_may_holds sigma b rho name x ->
    exists s, In s (gamma sigma) /\ atom_holds_concrete Kc s b rho name x.

Axiom forall_witness_family :
  forall sigma b rho x p,
    (forall v : nat, MayJudgment sigma b (update rho x v) p) ->
    forall v, exists s, In s (gamma sigma) /\
                        models Kc s b (update rho x v) p.

Axiom exists_witness :
  forall sigma b rho x p,
    (exists v, MayJudgment sigma b (update rho x v) p) ->
    exists v s, In s (gamma sigma) /\
                models Kc s b (update rho x v) p.

(* Additional axiom needed for the Forall case *)
Axiom forall_witness_uniform :
  forall sigma b rho x p,
    (forall v : nat, MayJudgment sigma b (update rho x v) p) ->
    exists s, In s (gamma sigma) /\ forall v, models Kc s b (update rho x v) p.

(* Soundness theorem *)
(* se nel dominio astratto posso derivare che la formula \phi "may hold", 
   allora esiste almeno un stato concreto s nella concretizzazione \gamma (\sigma) in cui la formula è vera nel concrete state *)
Theorem may_sound :
  forall (sigma0 : StateAbstract) (b0 : Block) (rho0 : Valuation) (phi : Formula),
    MayJudgment sigma0 b0 rho0 phi ->
    exists s : StateConcrete, In s (gamma sigma0) /\ models Kc s b0 rho0 phi.
Proof.
  (* induction H apre 8 sottocasi, 1 per ogni costruttore della definizione induttiva.
     Ogni caso corrisponde a una regola logica nel derivation system del judjment MAY *)
  intros sigma0 b0 rho0 phi H. (* prendo tutte le variabili e l'ipotesi H mi che dice che il giudizio astratto è derivabile*)
  induction H.  (* induzione sulla derivazione del judgment. Coq mi genererà un caso per ogni regola che puo' essere stata usata per derivare H.*)
  (* dim che Exists un s \in \gamma(\sigma) tale che models Kc s b \rho (FAtom name x). uso assioma che connette la proprietà astratta taint_may_holds c
     con un witness concreto:
     "Axiom atom_taint_concrete_witnes": l’assioma mi da che : Exists s tale che s \in \gamma(sigma) e la formula atomica è vera nel concreto
     Dato che models applicato ad atom è definito come atom_holds_concrete, ottengo la conclusion (con apply atom_taint_concrete_witness in H)*) 
  - (* MJ_Atom *)
    apply atom_taint_concrete_witness in H. (*Input: H dice "taint_may_holds sigma b rho name x"  Output: Trasforma H in "exists s concreto che soddisfa la formula atomica"*)
    destruct H as [s [Hin Hatom]]. (*Input: L'esistenziale appena creato Output: Estrae lo stato s, la prova che s ∈ gamma(sigma) (Hin) e che s soddisfa la formula Hatom*)
    exists s. split; [exact Hin|]. (*il mio witness è s e separe il goal in 2 parti. exact Hin: mi risolve la prima parte "s è in gamma(sigma)"*)
    simpl. exact Hatom. (* Semplifica models ... FAtom ... in atom_holds_concrete ... usola prova che ho già*)
    (*La premessa ha già un witness concreto s in cui not P è vero: qui la soundness c'è, la dim è fatta dato che esiste già s.*)
  - (* MJ_MayNot *) 
    destruct H as [s [Hin Hnotm]]. (*input: H è già un esistenziale della forma Exists s \in gamma(sigma). NOT models ...*)
    exists s. split; [exact Hin|]. (*output: estraggo direttamente il witness s, ritorno s*)
    simpl. exact Hnotm.
  - (* MJ_And *)
    (* Dalle ipotesi derivo 2 testimoni distinti: 1 per p ed 1 per q
    NB: Per la semantica di AND, mi serve un unico stato concreto s in cui entrambe le formule siano vere al tempo stesso.:
        * definisco un assioma di gluing (Axiom and_witness_exists): se posso dervire |-_may p e |-_may q sono a posto
      *)
    destruct (and_witness_exists sigma b rho p q H H0) as [s [Hin [Hm1 Hm2]]]. 
    (*Input: H e H0 sono le prove che p e q sono derivabili,
    Applica l'assioma che dice "se p e q sono derivabili, allora esiste un singolo stato che soddisfa entrambe"
    Output: estrae lo stato unificato s e le prove che soddisfa sia p che q*)
    exists s. split; [exact Hin|].
    simpl. split; assumption.
    (* dato che ho già un un witness per uno dei 2 casi disgiunti, allora esiste uno stato concreto in cui almeno uno è vero*)
  - (* MJ_OrL *)
    destruct IHMayJudgment as [s [Hin Hm]].
    exists s. split; [exact Hin|].
    simpl. left; assumption.
    (*simmetrico*)
  - (* MJ_OrR *)
    destruct IHMayJudgment as [s [Hin Hm]].
    exists s. split; [exact Hin|].
    simpl. right; assumption.
    (* deve esiste un singolo stato concreto s in gamma(sigma )tale che, per ogni valore v, \models Kc s b (update rho x v) p holds
      NB: devo prendere un s per ogni v, definisco assioma di  per l' uniformità del witness: Axiom forall_witness_uniform *)
  - (* MJ_Forall *)
    apply forall_witness_uniform in H.
    destruct H as [s [Hin Hm]].
    exists s. split; [exact Hin|].
    simpl. exact Hm.
  - (* MJ_Exists *)
    apply exists_witness in H.
    destruct H as [v [s [Hin Hm]]].
    exists s. split; [exact Hin|].
    simpl. exists v. exact Hm.
  - (* MJ_Then *)
    destruct H0 as [b' [Rb_plus Hq_may]]. (*Input: H0 dice \exists b'. R_B+ b b' AND MayJudgment ... q 
                                          Output: estraggo blocco b', la prova che è raggiungibile Rb+ e che q vale lì (Hq_may)*)
    apply then_witness_glue_plus with (b' := b') (q := q) in H; (*Input: H (prova di p), Rb+ (raggiungibilità), Hq_may (prova di q in b')
                                                                  Applico l'assioma che di gluing per mettere insieme i 2 witness 
                                                                  Output: un singolo stato s che soddisfa p in b e q in b'*)
      [|exact Rb_plus|exact Hq_may].
    destruct H as [s [Hin [Hm_p Hm_q]]].
    exists s. split; [exact Hin|]. 
    simpl. exists b'. split; [exact Rb_plus|split]; assumption. (*simpl: models ... (FThen p q) diventa exists b''. R_B+ b b'' AND models ... p AND models ... q
                                                                exists b': mi dice che il bb target è il b' che ho già
                                                                split: divido in 3 parti: raggiungibilità, p in b, q in b'
                                                                assumption: mi risolve tutto con le prove che ho gia definito*)
Qed.


(*Derived memory errors formulas and their soundness *)
Definition MemLeak (x : Var) : Formula :=
  FAnd (FAtom "alloc" x)
       (FNot (FThen (FAtom "alloc" x) (FAtom "drop" x))).

Definition DoubleFree (x : Var) : Formula :=
  FThen (FAtom "drop" x) (FAtom "drop" x).

Definition UseAfterFree (x : Var) : Formula :=
  FThen (FAtom "drop" x) (FAtom "use" x).

(* Memory Leak *)
Lemma mem_leak_sound :
  forall sigma b rho x,
    MayJudgment sigma b rho (MemLeak x) ->
    exists s, In s (gamma sigma) /\ models Kc s b rho (MemLeak x).
Proof.
  intros sigma b rho x H.
  inversion H; subst.
  pose proof (and_witness_exists sigma b rho
               (FAtom "alloc" x)
               (FNot (FThen (FAtom "alloc" x) (FAtom "drop" x)))
               H5 H6)
    as [s [Hin [Halloc Hnot]]].
  exists s. split; [exact Hin|simpl; split; assumption].
Qed.

(* Double Free *)
Lemma double_free_sound :
  forall sigma b rho x,
    MayJudgment sigma b rho (DoubleFree x) ->
    exists s, In s (gamma sigma) /\ models Kc s b rho (DoubleFree x).
Proof.
  intros sigma b rho x H. apply may_sound in H. exact H.
Qed.

(* UAF *)
Lemma use_after_free_sound :
  forall sigma b rho x,
    MayJudgment sigma b rho (UseAfterFree x) ->
    exists s, In s (gamma sigma) /\ models Kc s b rho (UseAfterFree x).
Proof.
  intros sigma b rho x H. apply may_sound in H. exact H.
Qed.


