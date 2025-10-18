(** {1 Diagrams} *)

module Paste_tree : sig
  type t = Leaf of Id.Tag.t | Node of int * t * t
end

(** {2 Core types} *)
type sign = [ `Input | `Output ]

type t = {
  shape: Ogposet.t;
  labels: Id.Tag.t array array;
  tree: sign -> int -> Paste_tree.t;
}

val shape : t -> Ogposet.t
val labels : t -> Id.Tag.t array array
val dim : t -> int
val is_round : t -> bool
val tree : t -> sign -> int -> Paste_tree.t

(** {2 Error-handling} *)
type error = Error.t

type 'a checked = 'a Error.checked

(** {2 Constructors} *)
val cell0 : Id.Tag.t -> t checked

val cellN : Id.Tag.t -> t -> t -> t checked
val paste : int -> t -> t -> t checked

(** {2 Derived operations} *)
val boundary : sign -> int -> t -> t

val label_set_of : t -> (Id.Tag.t * int) list
val isomorphic : t -> t -> bool
val equal : t -> t -> bool
val isomorphism_of : t -> t -> Ogposet.Embedding.t checked
val is_cell : t -> bool
