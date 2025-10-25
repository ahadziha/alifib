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
  | `Map
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
type t = { kind: kind; span: Positions.span }

let make kind span = { kind; span }
let kind t = t.kind
let span t = t.span
let is_trivia = function { kind= Trivia _; _ } -> true | _ -> false
let is_error = function { kind= Error _; _ } -> true | _ -> false

let classify = function
  | { kind= Trivia _; _ } ->
      `Trivia
  | { kind= Error _; _ } ->
      `Error
  | _ ->
      `Syntax

let keyword_of_string = function
  | "include" ->
      Some `Include
  | "attach" ->
      Some `Attach
  | "along" ->
      Some `Along
  | "assert" ->
      Some `Assert
  | "in" ->
      Some `In
  | "out" ->
      Some `Out
  | "Type" ->
      Some `Type
  | "let" ->
      Some `Let
  | "as" ->
      Some `As
  | "map" ->
      Some `Map
  | _ ->
      None

let comma_origin = function
  | { kind= Comma origin; _ } ->
      Some origin
  | _ ->
      None
