(** {1 Oriented graded posets (ogposets)} *)

(** {2 Core types} *)

type t
type poset = t
type sign = [ `Input | `Output | `Both ]

(** {2 Error handling} *)
type error = Error.t

type 'a checked = 'a Error.checked

(** {2 Sets of integers} *)
type intset

val intset_empty : intset
val intset_add : int -> intset -> intset
val intset_of_list : int list -> intset
val intset_of_array : int array -> intset
val intset_elements : intset -> int list

(** {2 Constructors} *)
val make :
  dim:int ->
  faces_in:intset array array ->
  faces_out:intset array array ->
  cofaces_in:intset array array ->
  cofaces_out:intset array array ->
  t

val empty : t
val point : t

(** {2 Accessors} *)
val is_normal : t -> bool

val dim : t -> int
val sizes : t -> int array
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

val extremal : sign -> int -> t -> intset
val maximal : int -> t -> intset
val is_pure : t -> bool
val is_atom : t -> bool
val boundary : sign -> int -> t -> t * Embedding.t

(** {2 Pushouts} *)

type pushout = { tip: t; inl: Embedding.t; inr: Embedding.t }

val pushout : Embedding.t -> Embedding.t -> pushout

(** {2 For molecules} *)

val is_round : t -> bool
val traverse : t -> (int * intset) list -> t * Embedding.t
val normalisation : t -> t * Embedding.t
val boundary_traverse : sign -> int -> t -> t * Embedding.t
val isomorphic : t -> t -> bool
val isomorphism_of : t -> t -> Embedding.t checked
