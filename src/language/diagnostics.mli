open Positions

type severity =
  [ `Error
  | `Warning
  | `Info
  ]

type producer = Error.Located.producer
type phase = Error.Located.phase

type message = { headline : string; details : string list }

type t = {
  severity : severity;
  payload : Error.Located.t;
  message : message;
  code : string option;
}

type diagnostic = t

val make :
  ?notes:string list ->
  ?details:string list ->
  ?code:string ->
  severity ->
  producer ->
  span ->
  string ->
  t

val of_error :
  ?code:string ->
  ?details:string list ->
  severity ->
  Error.Located.t ->
  t
val with_details : string list -> t -> t
val add_detail : string -> t -> t
val add_note : string -> t -> t
val map_error : (Error.t -> Error.t) -> t -> t
val span : t -> span
val producer : t -> producer
val to_error : t -> Error.t
val to_located_error : t -> Error.Located.t
val pp : Format.formatter -> t -> unit

type report = t list

module Report : sig
  type t = report

  val empty : t
  val add : diagnostic -> t -> t
  val append : t -> t -> t
  val pp : Format.formatter -> t -> unit
end
