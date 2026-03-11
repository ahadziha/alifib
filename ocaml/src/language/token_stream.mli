type t
type cursor

val create : source:Positions.Source.t -> string -> t
val source : t -> Positions.Source.t
val text : t -> string
val tokens : t -> Token.t array
val diagnostics : t -> Diagnostics.report
val reload : t -> unit
val apply_edit : t -> span:Positions.span -> replacement:string -> unit
val cursor : t -> cursor
val next : cursor -> Token.t option
val peek : cursor -> Token.t option
val reset : cursor -> unit
