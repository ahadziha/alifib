open Sexplib.Std

module type OrderedSexp = sig
  type t
  val compare : t -> t -> int
  val sexp_of_t : t -> Sexplib.Sexp.t
end

module MapWithSexp (Key : OrderedSexp) = struct
  include Map.Make(Key)
  let sexp_of_t sexp_of_v m =
    bindings m
    |> List.map (fun (k, v) -> (k, sexp_of_v v))
    |> [%sexp_of: (Key.t * Sexplib.Sexp.t) list]
end

module SetWithSexp (Elem : OrderedSexp) = struct  
  include Set.Make(Elem)
  let sexp_of_t s = [%sexp_of: Elem.t list] (elements s)
end

module ModuleMap = MapWithSexp (Id.Module)
module GlobalMap = MapWithSexp (Id.Global)
module GlobalSet = SetWithSexp (Id.Global)
module LocalMap = MapWithSexp (Id.Local)
module LocalSet = SetWithSexp (Id.Local)
module TagMap = MapWithSexp (Id.Tag)

module IntMap = MapWithSexp (struct
  include Int
  let sexp_of_t = Sexplib.Std.sexp_of_int
end)
