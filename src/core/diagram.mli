type t = {
  shape: Ogposet.t;
  labels: Id.tag array array;
}

type error = {
  message: string;
  notes: string list;
}

val error : ?notes:string list -> string -> error
val pp_error : Format.formatter -> error -> unit
type 'a checked = ('a, error) result

val shape : t -> Ogposet.t
val labels : t -> Id.tag array array
val dim : t -> int
val is_round : t -> bool

val cell0 : Id.tag -> t checked
