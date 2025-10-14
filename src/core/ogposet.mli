(** {1 Oriented graded posets (ogposets)}

    Minimal internal data structure with embeddings and standard categorical
    constructions. No well-formedness checks are performed; this module is
    designed for use inside verified higher-level code. *)

(** {2 Core types} *)

(** An oriented graded poset (ogposet). *)
type t

type poset = t

(** Abstract type of sets of integers used for faces and cofaces. *)
type intset

(** Orientation: input, output, or both. *)
type sign = [ `Input | `Output | `Both ]

(** Build an ogposet directly from adjacency arrays of integer sets and its
    dimension. *)
val make :
  dim:int ->
  faces_in:intset array array ->
  faces_out:intset array array ->
  cofaces_in:intset array array ->
  cofaces_out:intset array array ->
  t

val empty : t
val point : t
val dim : t -> int
val sizes : t -> int array

(** Access the input/output/both faces or cofaces of a given element. *)
val faces_of : sign -> t -> dim:int -> pos:int -> intset

val cofaces_of : sign -> t -> dim:int -> pos:int -> intset
val equal : t -> t -> bool

(** {2 Embeddings} *)

module Embedding : sig
  type t

  val make :
    dom:poset -> cod:poset -> map:int array array -> inv:int array array -> t

  val dom : t -> poset
  val cod : t -> poset
  val map : t -> int array array
  val inv : t -> int array array
  val empty : poset -> t
  val id : poset -> t
  val compose : t -> t -> t
end

(** {2 Boundary operations} *)

(** extremal `Input k g` = k-cells with no output cofaces, extremal `Output k g`
    = k-cells with no input cofaces, extremal `Both k g` = union of the two *)
val extremal : sign -> int -> t -> intset

val maximal : int -> t -> intset
val is_pure : t -> bool

(** roundness predicate; to be run exclusively on molecules *)
val is_round : t -> bool

(** [boundary sign at_dim X] is the embedding of the appropriate boundary of X
    into it *)
val boundary : sign -> int -> t -> t * Embedding.t
val traverse : t -> (int * intset) list -> t * Embedding.t

(** {2 Pushouts} *)

type pushout = { tip: t; inl: Embedding.t; inr: Embedding.t }

val pushout : Embedding.t -> Embedding.t -> pushout
