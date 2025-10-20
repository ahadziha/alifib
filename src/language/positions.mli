module Source : sig
  type t = Id.Module.t

  val of_path : string -> t
  val virtual_ : string -> t
  val unknown : t
  val to_string : t -> string
  val equal : t -> t -> bool
  val compare : t -> t -> int
  val hash : t -> int
  val pp : Format.formatter -> t -> unit
end

type point = {
  source: Source.t;
  offset: int;
  line: int;
  column: int;
  bol_offset: int;
}

val make_point :
  source:Source.t ->
  offset:int ->
  line:int ->
  column:int ->
  bol_offset:int ->
  point

val unknown_point : point
val compare_point : point -> point -> int
val pp_point : Format.formatter -> point -> unit
val advance : point -> string -> point
val with_source : point -> Source.t -> point

type span = { start: point; stop: point }

val make_span : start:point -> stop:point -> span
val point_span : point -> span
val merge : span -> span -> span
val contains : span -> point -> bool
val length : span -> int
val is_point : span -> bool
val between : point -> point -> span option
val pp_span : Format.formatter -> span -> unit
val point_of_lexing : Source.t -> Lexing.position -> point
val to_lexing : point -> Lexing.position
val span_of_lexing : Source.t -> Lexing.position -> Lexing.position -> span
val to_error_span : span -> Error.Located.Span.t
val of_error_span : Error.Located.Span.t -> span
