module Report = Diagnostics.Report
module I = Parser.MenhirInterpreter

let parser_producer =
  { Error.Located.phase= `Parser; module_path= Some "language.parser" }

type pretoken =
  | Single of Token.t
  | Dot_selector of Token.t * Token.t

let positions_of_token token =
  let span = Token.span token in
  let startp = Positions.to_lexing span.start in
  let endp = Positions.to_lexing span.stop in
  (startp, endp)

let positions_of_pair first second =
  let startp, _ = positions_of_token first in
  let _, endp = positions_of_token second in
  (startp, endp)

let is_dot token =
  match Token.kind token with
  | Token.Dot ->
      true
  | _ ->
      false

let is_selector_kw token =
  match Token.kind token with
  | Token.Keyword (`In | `Out) ->
      true
  | _ ->
      false

let rec decode_tokens = function
  | dot :: kw :: rest when is_dot dot && is_selector_kw kw ->
      Dot_selector (dot, kw) :: decode_tokens rest
  | token :: rest ->
      Single token :: decode_tokens rest
  | [] ->
      []

let to_parser_token pretoken =
  match pretoken with
  | Single token ->
      let startp, endp = positions_of_token token in
      begin
        match Token.kind token with
        | Token.Eof ->
            Some (Parser.EOF, startp, endp)
        | Token.At ->
            Some (Parser.AT token, startp, endp)
        | Token.Keyword `Type ->
            Some (Parser.KW_TYPE token, startp, endp)
        | Token.Keyword `Include ->
            Some (Parser.KW_INCLUDE token, startp, endp)
        | Token.Keyword `Attach ->
            Some (Parser.KW_ATTACH token, startp, endp)
        | Token.Keyword `Along ->
            Some (Parser.KW_ALONG token, startp, endp)
        | Token.Keyword `Assert ->
            Some (Parser.KW_ASSERT token, startp, endp)
        | Token.Keyword `Let ->
            Some (Parser.KW_LET token, startp, endp)
        | Token.Keyword `As ->
            Some (Parser.KW_AS token, startp, endp)
        | Token.Keyword (`In | `Out) ->
            None
        | Token.Identifier _ ->
            Some (Parser.IDENT token, startp, endp)
        | Token.Nat _ ->
            Some (Parser.NAT token, startp, endp)
        | Token.L_brace ->
            Some (Parser.LBRACE token, startp, endp)
        | Token.R_brace ->
            Some (Parser.RBRACE token, startp, endp)
        | Token.L_bracket ->
            Some (Parser.LBRACKET token, startp, endp)
        | Token.R_bracket ->
            Some (Parser.RBRACKET token, startp, endp)
        | Token.L_paren ->
            Some (Parser.LPAREN token, startp, endp)
        | Token.R_paren ->
            Some (Parser.RPAREN token, startp, endp)
        | Token.Comma _ ->
            Some (Parser.COMMA token, startp, endp)
        | Token.Dot ->
            Some (Parser.DOT token, startp, endp)
        | Token.Paste ->
            Some (Parser.PASTE token, startp, endp)
        | Token.Colon ->
            Some (Parser.COLON token, startp, endp)
        | Token.Of_shape ->
            Some (Parser.OF_SHAPE token, startp, endp)
        | Token.Maps_to ->
            Some (Parser.MAPS_TO token, startp, endp)
        | Token.Arrow ->
            Some (Parser.ARROW token, startp, endp)
        | Token.Has_value ->
            Some (Parser.HAS_VALUE token, startp, endp)
        | Token.Equal ->
            Some (Parser.EQUAL token, startp, endp)
        | Token.Hole ->
            Some (Parser.HOLE token, startp, endp)
        | Token.Trivia _ ->
            None
        | Token.Error _ ->
            None
      end
  | Dot_selector (dot_token, kw_token) ->
      let startp, endp = positions_of_pair dot_token kw_token in
      Some (Parser.DOT_SELECTOR (dot_token, kw_token), startp, endp)

let add_parse_error diagnostics message =
  let span = Positions.point_span Positions.unknown_point in
  let diag = Diagnostics.make `Error parser_producer span message in
  Report.add diag diagnostics

let rec loop checkpoint tokens =
  match checkpoint with
  | I.InputNeeded _ -> (
      match tokens with
      | (token, startp, endp) :: rest ->
          let checkpoint = I.offer checkpoint (token, startp, endp) in
          loop checkpoint rest
      | [] ->
          let pos = Lexing.dummy_pos in
          let checkpoint = I.offer checkpoint (Parser.EOF, pos, pos) in
          loop checkpoint [])
  | I.Shifting _ | I.AboutToReduce _ ->
      loop (I.resume checkpoint) tokens
  | I.Accepted ast ->
      ast
  | I.HandlingError _ ->
      failwith "unexpected parser error"
  | I.Rejected ->
      failwith "input rejected"

let parse stream =
  let base_diagnostics = Token_stream.diagnostics stream in
  let diagnostics = ref base_diagnostics in
  let raw_tokens = Array.to_list (Token_stream.tokens stream) in
  let syntactic_tokens =
    raw_tokens |> List.filter (fun token -> not (Token.is_trivia token))
  in
  let decoded = decode_tokens syntactic_tokens in
  let tokens = decoded |> List.filter_map to_parser_token in
  let start_pos = Lexing.dummy_pos in
  let checkpoint = Parser.Incremental.program start_pos in
  let ast =
    try loop checkpoint tokens
    with Failure message ->
      diagnostics := add_parse_error !diagnostics message
      ; Ast.empty
  in
  (ast, !diagnostics)
