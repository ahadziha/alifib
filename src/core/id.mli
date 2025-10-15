(** Unique global identifiers for cells and complexes. *)

type t

val fresh : unit -> t
val equal : t -> t -> bool
val compare : t -> t -> int
val hash : t -> int
val to_int : t -> int
val pp : Format.formatter -> t -> unit

(** {2 Human-readable names} *)

type error = {
  message: string;
  notes: string list;
}

val error : ?notes:string list -> string -> error
val pp_error : Format.formatter -> error -> unit
type 'a checked = ('a, error) result

module Name : sig
  type simple
  type t

  val simple : string -> simple checked
  val simple_to_string : simple -> string
  val make : string -> t checked
  val to_string : t -> string
end

(** {2 Tags} *)

type tag = [ `Local of Name.t | `Global of t ]

val tag_equal : tag -> tag -> bool
val tag_compare : tag -> tag -> int
val tag_pp : Format.formatter -> tag -> unit
