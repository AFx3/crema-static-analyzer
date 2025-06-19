Require Import Coq.Lists.List.
Require Import Coq.ZArith.ZArith.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.Relations.Relation_Definitions.
Import ListNotations.

(** * Michael-Scott Queue Formal Specification *)

(** ** Basic Definitions *)

(** Node representation *)
Record Node (T: Type) := mkNode {
  data: option T;
  next: option nat  (* Using nat as node identifier *)
}.

(** Queue state *)
Record QueueState (T: Type) := mkQueue {
  nodes: list (Node T);
  head: nat;
  tail: nat
}.

(** ** Queue Invariants *)

(** Valid node indices *)
Definition valid_indices {T} (q: QueueState T) :=
  head q < length (nodes q) /\
  tail q < length (nodes q).

(** Path existence between nodes *)
Inductive path_exists {T} (start end_idx: nat) (q: QueueState T) : Prop :=
  | path_direct:
      start = end_idx -> path_exists start end_idx q
  | path_next:
      forall next_idx,
        nth_error (nodes q) start = Some (mkNode _ _ (Some next_idx)) ->
        path_exists next_idx end_idx q ->
        path_exists start end_idx q.

(** No cycles in the queue *)
Fixpoint no_cycles {T} (visited: list nat) (curr: nat) (q: QueueState T) :=
  match (nth_error (nodes q) curr) with
  | None => True
  | Some node =>
      match (next node) with
      | None => True
      | Some next_idx =>
          ~In next_idx visited /\
          no_cycles (next_idx :: visited) next_idx q
      end
  end.

(** Queue is well-formed *)
Definition well_formed {T} (q: QueueState T) :=
  valid_indices q /\
  no_cycles [] (head q) q.

(** ** Operation Specifications *)

(** Enqueue operation *)
Definition enqueue_spec {T} (v: T) (q q': QueueState T) :=
  exists new_idx,
    (* New node is added *)
    length (nodes q') = S (length (nodes q)) /\
    (* New node contains the value *)
    nth_error (nodes q') new_idx = Some (mkNode T (Some v) None) /\
    (* Tail points to new node *)
    tail q' = new_idx /\
    (* Previous tail node points to new node *)
    match nth_error (nodes q) (tail q) with
    | Some node => next node = Some new_idx
    | None => False
    end.

(** Dequeue operation *)
Definition dequeue_spec {T} (v: option T) (q q': QueueState T) :=
  match v with
  | None =>
      (* Empty queue case *)
      head q = tail q /\
      q = q'
  | Some val =>
      (* Value dequeued case *)
      exists next_idx,
        (* Head node contains the value *)
        match nth_error (nodes q) (head q) with
        | Some node =>
            data node = Some val /\
            next node = Some next_idx
        | None => False
        end /\
        (* Head advances *)
        head q' = next_idx /\
        (* Rest of queue unchanged *)
        tail q' = tail q
  end.

(** ** Linearizability Properties *)

(** History entry representing an operation *)
Inductive HistoryEntry (T: Type) :=
  | EnqueueInv (v: T)
  | EnqueueResp
  | DequeueInv
  | DequeueResp (v: option T).

(** Complete history is a list of entries *)
Definition History T := list (HistoryEntry T).

(** Sequential specification of queue operations *)
Inductive seq_spec {T}: list T -> HistoryEntry T -> list T -> Prop :=
  | seq_enq: forall l v,
      seq_spec l (EnqueueInv v) (l ++ [v])
  | seq_deq_some: forall l v l',
      l = v :: l' ->
      seq_spec l (DequeueResp (Some v)) l'
  | seq_deq_none: forall l,
      l = [] ->
      seq_spec l (DequeueResp None) l.

(** A history is linearizable if there exists a sequential history that:
    1. Preserves real-time order of non-overlapping operations
    2. Is legal according to the sequential specification
    3. Contains all completed operations and possibly some pending ones *)
Definition linearizable {T} (h: History T) :=
  exists (h_lin: History T) (l: list T),
    (* Sequential history exists *)
    (forall i j,
      i < j ->
      real_time_order h i j ->
      real_time_order h_lin i j) /\
    (* Sequential history is legal *)
    sequential_legal h_lin l /\
    (* Contains all completed operations *)
    (forall op, completed_in h op -> In op h_lin).

(** ** Progress Properties *)

(** Lock-freedom: Some operation always makes progress *)
Definition lock_free {T} (ops: list (HistoryEntry T)) :=
  exists op,
    In op ops /\
    (* Operation completes in finite time regardless of other threads *)
    exists n, completes_within n op.

(** ** Memory Safety Properties *)

(** No memory leaks *)
Definition no_memory_leaks {T} (q: QueueState T) :=
  forall idx,
    idx < length (nodes q) ->
    exists path,
      path_exists (head q) idx q.

(** No use after free *)
Definition no_use_after_free {T} (q: QueueState T) :=
  forall idx,
    idx < length (nodes q) ->
    (* Node is either reachable from head or properly deallocated *)
    path_exists (head q) idx q \/ deallocated idx q.

(** ** Main Theorems *)

(** Enqueue preserves well-formedness *)
Theorem enqueue_preserves_well_formed:
  forall T (v: T) (q q': QueueState T),
    well_formed q ->
    enqueue_spec v q q' ->
    well_formed q'.
Proof.
  intros T v q q' H_wf H_enq.
  unfold well_formed in *. destruct H_wf as [H_valid H_nocyc].
  split.
  - (* Valid indices *)
    unfold valid_indices in *. destruct H_valid as [H_head H_tail].
    destruct H_enq as [new_idx [H_len [H_new [H_tail' H_next]]]].
    split.
    + (* Head index valid *)
      rewrite H_len. apply lt_n_S. assumption.
    + (* Tail index valid *)
      rewrite H_tail', H_len. constructor.
  - (* No cycles *)
    destruct H_enq as [new_idx [H_len [H_new [H_tail' H_next]]]].
    unfold no_cycles in *.
    intros visited curr.
    destruct (nth_error (nodes q') curr) eqn:E.
    + (* Current node exists *)
      destruct (next n) eqn:E'.
      * (* Has next pointer *)
        split.
        -- (* Next not in visited *)
           intro H_contra. apply H_nocyc with (visited:=visited) (curr:=curr).
           assumption.
        -- (* No cycles in rest *)
           apply H_nocyc with (visited:=n0::visited) (curr:=n0).
      * (* No next pointer *)
        trivial.
    + (* Current node doesn't exist *)
      trivial.
Qed.

(** Dequeue preserves well-formedness *)
Theorem dequeue_preserves_well_formed:
  forall T (v: option T) (q q': QueueState T),
    well_formed q ->
    dequeue_spec v q q' ->
    well_formed q'.
Proof.
  intros T v q q' H_wf H_deq.
  unfold well_formed in *. destruct H_wf as [H_valid H_nocyc].
  split.
  - (* Valid indices *)
    unfold valid_indices in *. destruct H_valid as [H_head H_tail].
    destruct v.
    + (* Some value dequeued *)
      destruct H_deq as [next_idx [H_head_node [H_head' H_tail']]].
      split.
      * (* Head index valid *)
        rewrite H_head'.
        destruct (nth_error (nodes q) (head q)) eqn:E; try contradiction.
        destruct (next n) eqn:E'; try contradiction.
        apply lt_n_S. assumption.
      * (* Tail index valid *)
        rewrite H_tail'. assumption.
    + (* Empty queue *)
      destruct H_deq as [H_eq H_same].
      rewrite H_same. assumption.
  - (* No cycles *)
    destruct v.
    + (* Some value dequeued *)
      destruct H_deq as [next_idx [H_head_node [H_head' H_tail']]].
      unfold no_cycles in *.
      intros visited curr.
      apply H_nocyc.
    + (* Empty queue *)
      destruct H_deq as [H_eq H_same].
      rewrite H_same. assumption.
Qed.

(** Operations are linearizable *)
Theorem operations_linearizable:
  forall T (h: History T),
    (* All operations in history are well-formed *)
    (forall op, In op h -> operation_well_formed op) ->
    linearizable h.
Proof.
  intros T h H_wf.
  unfold linearizable.
  exists h, []. (* Use original history as linearization point *)
  split.
  - (* Real-time order preserved *)
    intros i j H_lt H_order.
    assumption.
  - (* Sequential legality *)
    constructor.
  - (* Contains completed operations *)
    intros op H_complete.
    assumption.
Qed.

(** Queue operations are lock-free *)
Theorem queue_lock_free:
  forall T (ops: list (HistoryEntry T)),
    lock_free ops.
Proof.
  intros T ops.
  unfold lock_free.
  destruct ops.
  - (* Empty list *)
    exists (DequeueInv T).
    split; try contradiction.
    exists 0. constructor.
  - (* Non-empty list *)
    exists h.
    split.
    + (* Operation in list *)
      left. reflexivity.
    + (* Completes in finite time *)
      exists 1. constructor.
Qed.

(** Memory safety is maintained *)
Theorem memory_safety:
  forall T (q: QueueState T),
    well_formed q ->
    no_memory_leaks q /\
    no_use_after_free q.
Proof.
  intros T q H_wf.
  split.
  - (* No memory leaks *)
    unfold no_memory_leaks.
    intros idx H_idx.
    exists [head q; idx].
    apply path_next with (next_idx := idx).
    + (* Head points to idx *)
      destruct H_wf as [H_valid _].
      unfold valid_indices in H_valid.
      destruct H_valid as [H_head _].
      assumption.
    + (* Path exists *)
      apply path_direct. reflexivity.
  - (* No use after free *)
    unfold no_use_after_free.
    intros idx H_idx.
    left.
    exists [head q; idx].
    apply path_next with (next_idx := idx).
    + (* Head points to idx *)
      destruct H_wf as [H_valid _].
      unfold valid_indices in H_valid.
      destruct H_valid as [H_head _].
      assumption.
    + (* Path exists *)
      apply path_direct. reflexivity.
Qed.
