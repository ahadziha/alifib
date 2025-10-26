module Report = Diagnostics.Report
module I = Parser.MenhirInterpreter

let parser_producer =
  { Error.Located.phase= `Parser; module_path= Some "language.parser" }

let positions_of_token token =
  let span = Token.span token in
  let startp = Positions.to_lexing span.start in
  let endp = Positions.to_lexing span.stop in
  (startp, endp)

let to_parser_token token =
  if Token.is_trivia token || Token.is_error token then None
  else
    let startp, endp = positions_of_token token in
    let open Parser in
    match Token.kind token with
    | Token.Eof ->
        Some (EOF, startp, endp)
    | Token.At ->
        Some (AT token, startp, endp)
    | Token.Keyword kw -> (
        match kw with
        | `Type ->
            Some (TYPE token, startp, endp)
        | `Include ->
            Some (INCLUDE token, startp, endp)
        | `Attach ->
            Some (ATTACH token, startp, endp)
        | `Along ->
            Some (ALONG token, startp, endp)
        | `Map ->
            Some (MAP token, startp, endp)
        | `Assert ->
            Some (ASSERT token, startp, endp)
        | `In ->
            Some (IN token, startp, endp)
        | `Out ->
            Some (OUT token, startp, endp)
        | `Let ->
            Some (LET token, startp, endp)
        | `As ->
            Some (AS token, startp, endp))
    | Token.Identifier _ ->
        Some (IDENT token, startp, endp)
    | Token.Nat _ ->
        Some (NAT token, startp, endp)
    | Token.L_brace ->
        Some (LBRACE token, startp, endp)
    | Token.R_brace ->
        Some (RBRACE token, startp, endp)
    | Token.L_bracket ->
        Some (LBRACKET token, startp, endp)
    | Token.R_bracket ->
        Some (RBRACKET token, startp, endp)
    | Token.L_paren ->
        Some (LPAREN token, startp, endp)
    | Token.R_paren ->
        Some (RPAREN token, startp, endp)
    | Token.Comma _ ->
        Some (COMMA token, startp, endp)
    | Token.Dot ->
        Some (DOT token, startp, endp)
    | Token.Paste ->
        Some (PASTE token, startp, endp)
    | Token.Colon ->
        Some (COLON token, startp, endp)
    | Token.Of_shape ->
        Some (OF_SHAPE token, startp, endp)
    | Token.Maps_to ->
        Some (MAPS_TO token, startp, endp)
    | Token.Arrow ->
        Some (ARROW token, startp, endp)
    | Token.Has_value ->
        Some (HAS_VALUE token, startp, endp)
    | Token.Equal ->
        Some (EQUAL token, startp, endp)
    | Token.Hole ->
        Some (HOLE token, startp, endp)
    | Token.Trivia _ | Token.Error _ ->
        None

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
  let tokens =
    let is_comma token =
      match Token.kind token with Token.Comma _ -> true | _ -> false
    in
    let drops_after token =
      match Token.kind token with
      | Token.At | Token.R_brace | Token.R_bracket | Token.Eof ->
          true
      | Token.Comma _ ->
          true
      | _ ->
          false
    in
    let filter_commas tokens =
      let rec aux acc = function
        | comma :: (next :: _ as rest) when is_comma comma ->
            if drops_after next then aux acc rest else aux (comma :: acc) rest
        | token :: rest ->
            aux (token :: acc) rest
        | [] ->
            List.rev acc
      in
      aux [] tokens
    in
    raw_tokens
    |> List.filter (fun token -> not (Token.is_trivia token))
    |> filter_commas
    |> List.filter_map to_parser_token
  in
  let start_pos = Lexing.dummy_pos in
  let checkpoint = Parser.Incremental.program start_pos in
  let ast =
    try loop checkpoint tokens
    with Failure message ->
      diagnostics := add_parse_error !diagnostics message
      ; Ast.empty
  in
  (ast, !diagnostics)
