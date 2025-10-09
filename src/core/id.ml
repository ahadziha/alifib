type t = int
let counter = ref 0
let fresh () = let i = !counter in incr counter; i
let equal = Int.equal
let compare = Int.compare
let hash = Hashtbl.hash
let to_int x = x
let pp fmt x = Format.fprintf fmt "#%d" x
