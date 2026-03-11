type error = Error.t
type 'a checked = 'a Error.checked

module Global : sig
  type t [@@deriving sexp_of]

  val fresh : unit -> t
  val equal : t -> t -> bool
  val compare : t -> t -> int
  val hash : t -> int
  val to_int : t -> int
  val pp : Format.formatter -> t -> unit
end

module Local : sig
  type t [@@deriving sexp_of]

  val make : string -> t
  val to_string : t -> string
  val equal : t -> t -> bool
  val compare : t -> t -> int
  val hash : t -> int
  val pp : Format.formatter -> t -> unit
end

module Module : sig
  type t [@@deriving sexp_of]

  val of_path : string -> t
  val to_string : t -> string
  val equal : t -> t -> bool
  val compare : t -> t -> int
  val hash : t -> int
  val pp : Format.formatter -> t -> unit
end

module Tag : sig
  type t = [ `Local of Local.t | `Global of Global.t ] [@@deriving sexp_of]

  val equal : t -> t -> bool
  val compare : t -> t -> int
  val pp : Format.formatter -> t -> unit
  val of_local : Local.t -> t
  val of_global : Global.t -> t
end
