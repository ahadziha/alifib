(** {1 Diagrams} *)

(** {2 Core types} *)
module Paste_tree : sig
  type t = Leaf of Id.Tag.t | Node of int * t * t
end

type sign = [ `Input | `Output ]

type t = {
  shape: Ogposet.t;
  labels: Id.Tag.t array array;
  tree: sign -> int -> Paste_tree.t;
}

type cell_data =
  | Zero
  | Boundary of { boundary_in: t; boundary_out: t }

(** {2 Accessors} *)
val shape : t -> Ogposet.t

val labels : t -> Id.Tag.t array array
val dim : t -> int
val tree : t -> sign -> int -> Paste_tree.t

(** {2 Error-handling} *)
type error = Error.t

type 'a checked = 'a Error.checked

(** {2 Constructors} *)
val cell : Id.Tag.t -> cell_data -> t checked
val paste : int -> t -> t -> t checked

(** {2 Basic utilities} *)
val is_round : t -> bool

val parallelism :
  t -> t -> (Ogposet.t * Ogposet.Embedding.t * Ogposet.Embedding.t) checked

val parallel : t -> t -> bool

val pastability :
  int ->
  t ->
  t ->
  (Ogposet.t * Ogposet.Embedding.t * Ogposet.Embedding.t) checked

val pastable : int -> t -> t -> bool
val normal : t -> t
val is_normal : t -> bool
val boundary : sign -> int -> t -> t checked
val boundary_normal : sign -> int -> t -> t checked
val is_cell : t -> bool
val label_set_of : t -> (Id.Tag.t * int) list
val has_local_labels : t -> bool

(** {2 Isomorphism} *)
val isomorphic : t -> t -> bool

val equal : t -> t -> bool
val isomorphism_of : t -> t -> Ogposet.Embedding.t checked
