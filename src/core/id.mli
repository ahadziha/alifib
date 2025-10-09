(** Unique global identifiers for cells and complexes. *)

type t
val fresh : unit -> t
val equal : t -> t -> bool
val compare : t -> t -> int
val hash : t -> int
val to_int : t -> int
val pp : Format.formatter -> t -> unit
