type t = {
  shape: Ogposet.t;
  labels: Id.tag array array;
}

val shape : t -> Ogposet.t
val labels : t -> Id.tag array array
val dim : t -> int
val is_round : t -> bool
