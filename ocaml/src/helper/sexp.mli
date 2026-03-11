module type OrderedSexp = sig
  type t
  val compare : t -> t -> int
  val sexp_of_t : t -> Sexplib.Sexp.t
end

module MapWithSexp (Key : OrderedSexp) : sig
  include Map.S with type key = Key.t
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end

module SetWithSexp (Elem : OrderedSexp) : sig
  include Set.S with type elt = Elem.t
  val sexp_of_t : t -> Sexplib.Sexp.t
end

module ModuleMap : sig
  include Map.S with type key = Id.Module.t
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end

module GlobalMap : sig
  include Map.S with type key = Id.Global.t
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end

module GlobalSet : sig
  include Set.S with type elt = Id.Global.t
  val sexp_of_t : t -> Sexplib.Sexp.t
end

module LocalMap : sig
  include Map.S with type key = Id.Local.t
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end

module LocalSet : sig
  include Set.S with type elt = Id.Local.t
  val sexp_of_t : t -> Sexplib.Sexp.t
end

module TagMap : sig
  include Map.S with type key = Id.Tag.t
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end

module IntMap : sig
  include Map.S with type key = int
  val sexp_of_t : ('a -> Sexplib.Sexp.t) -> 'a t -> Sexplib.Sexp.t
end
