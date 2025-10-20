type t = { message: string; notes: string list }
type error = t

val make : ?notes:string list -> string -> t
val pp : Format.formatter -> t -> unit

type 'a checked = ('a, t) result

module Located : sig
  type phase = [ `Lexer | `Parser | `Driver | `Interpreter | `Other of string ]
  type producer = { phase: phase; module_path: string option }

  module Span : sig
    type t = { start: Lexing.position; stop: Lexing.position }

    val make : Lexing.position -> Lexing.position -> t
    val point : Lexing.position -> t
  end

  type t = { error: error; span: Span.t; producer: producer }

  val make : ?notes:string list -> producer -> Span.t -> string -> t
  val attach : producer -> Span.t -> error -> t
  val map_error : (error -> error) -> t -> t
  val to_error : t -> error
  val pp : Format.formatter -> t -> unit
end

type located = Located.t
