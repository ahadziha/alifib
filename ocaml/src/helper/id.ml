open Sexplib.Std

type error = Error.t
type 'a checked = 'a Error.checked

module Global = struct
  type t = int [@@deriving sexp_of]

  let counter = ref 0

  let fresh () =
    let i = !counter in
    incr counter ; i

  let equal = Int.equal
  let compare = Int.compare
  let hash = Hashtbl.hash
  let to_int x = x
  let pp fmt x = Format.fprintf fmt "#%d" x
end

module Local = struct
  type t = string [@@deriving sexp_of]

  let make s = s
  let to_string s = s
  let equal = String.equal
  let compare = String.compare
  let hash = Hashtbl.hash
  let pp fmt s = Format.fprintf fmt "%s" s
end

module Module = struct
  type t = string [@@deriving sexp_of]

  let of_path path = path
  let to_string id = id
  let equal = String.equal
  let compare = String.compare
  let hash = Hashtbl.hash
  let pp fmt id = Format.fprintf fmt "%s" id
end

module Tag = struct
  type t = [ `Local of Local.t | `Global of Global.t ] [@@deriving sexp_of]

  let equal a b =
    match (a, b) with
    | `Local x, `Local y ->
        Local.equal x y
    | `Global x, `Global y ->
        Global.equal x y
    | (`Local _ | `Global _), _ ->
        false

  let compare a b =
    match (a, b) with
    | `Local x, `Local y ->
        Local.compare x y
    | `Global x, `Global y ->
        Global.compare x y
    | `Local _, `Global _ ->
        -1
    | `Global _, `Local _ ->
        1

  let pp fmt = function
    | `Local name ->
        Format.fprintf fmt "%s" (Local.to_string name)
    | `Global id ->
        Global.pp fmt id

  let of_local name = `Local name
  let of_global id = `Global id
end
