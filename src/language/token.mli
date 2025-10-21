type comma_origin = [ `Explicit | `From_newline ]
type keyword =
  [ `Include
  | `Attach
  | `Along
  | `Assert
  | `In
  | `Out
  | `Type
  | `Let
  | `As
  ]
type trivia = Whitespace of string | Newline of string | Comment of string

type kind =
  | At
  | Keyword of keyword
  | Identifier of string
  | Nat of string
  | L_brace
  | R_brace
  | L_bracket
  | R_bracket
  | L_paren
  | R_paren
  | Comma of comma_origin
  | Dot
  | Paste
  | Colon
  | Of_shape
  | Maps_to
  | Arrow
  | Has_value
  | Equal
  | Hole
  | Trivia of trivia
  | Error of string
  | Eof

type classification = [ `Trivia | `Error | `Syntax ]
type t = private { kind: kind; span: Positions.span }

val make : kind -> Positions.span -> t
val kind : t -> kind
val span : t -> Positions.span
val is_trivia : t -> bool
val is_error : t -> bool
val classify : t -> classification
val keyword_of_string : string -> keyword option
val comma_origin : t -> comma_origin option
