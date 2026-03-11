module Report = Diagnostics.Report
module I = Parser.MenhirInterpreter
module Pos = Positions

type parser_token = {
  menhir: Parser.token;
  original: Token.t option;
  startp: Lexing.position;
  endp: Lexing.position;
}

exception Parse_error of parser_token option

let parser_producer =
  { Error.Located.phase= `Parser; module_path= Some "language.parser" }

let positions_of_token token =
  let span = Token.span token in
  let startp = Pos.to_lexing span.start in
  let endp = Pos.to_lexing span.stop in
  (startp, endp)

let to_parser_token token =
  if Token.is_trivia token || Token.is_error token then None
  else
    let startp, endp = positions_of_token token in
    let open Parser in
    let make menhir = Some { menhir; original= Some token; startp; endp } in
    match Token.kind token with
    | Token.Eof ->
        Some { menhir= EOF; original= Some token; startp; endp }
    | Token.At ->
        make (AT token)
    | Token.Keyword kw -> (
        match kw with
        | `Type ->
            make (TYPE token)
        | `Include ->
            make (INCLUDE token)
        | `Attach ->
            make (ATTACH token)
        | `Along ->
            make (ALONG token)
        | `Map ->
            make (MAP token)
        | `Assert ->
            make (ASSERT token)
        | `In ->
            make (IN token)
        | `Out ->
            make (OUT token)
        | `Let ->
            make (LET token)
        | `As ->
            make (AS token))
    | Token.Identifier _ ->
        make (IDENT token)
    | Token.Nat _ ->
        make (NAT token)
    | Token.L_brace ->
        make (LBRACE token)
    | Token.R_brace ->
        make (RBRACE token)
    | Token.L_bracket ->
        make (LBRACKET token)
    | Token.R_bracket ->
        make (RBRACKET token)
    | Token.L_paren ->
        make (LPAREN token)
    | Token.R_paren ->
        make (RPAREN token)
    | Token.Comma _ ->
        make (COMMA token)
    | Token.Dot ->
        make (DOT token)
    | Token.Paste ->
        make (PASTE token)
    | Token.Colon ->
        make (COLON token)
    | Token.Of_shape ->
        make (OF_SHAPE token)
    | Token.Maps_to ->
        make (MAPS_TO token)
    | Token.Arrow ->
        make (ARROW token)
    | Token.Has_value ->
        make (HAS_VALUE token)
    | Token.Equal ->
        make (EQUAL token)
    | Token.Hole ->
        make (HOLE token)
    | Token.Trivia _ | Token.Error _ ->
        None

let add_parse_error diagnostics span message =
  let diag = Diagnostics.make `Error parser_producer span message in
  Report.add diag diagnostics

let describe_keyword = function
  | `Include ->
      "'include'"
  | `Attach ->
      "'attach'"
  | `Along ->
      "'along'"
  | `Assert ->
      "'assert'"
  | `In ->
      "'in'"
  | `Out ->
      "'out'"
  | `Type ->
      "'Type'"
  | `Let ->
      "'let'"
  | `As ->
      "'as'"
  | `Map ->
      "'map'"

let describe_token token =
  match Token.kind token with
  | Token.Keyword kw ->
      Printf.sprintf "keyword %s" (describe_keyword kw)
  | Token.Identifier ident ->
      Printf.sprintf "identifier %S" ident
  | Token.Nat digits ->
      Printf.sprintf "number %s" digits
  | Token.At ->
      "'@'"
  | Token.L_brace ->
      "'{'"
  | Token.R_brace ->
      "'}'"
  | Token.L_bracket ->
      "'['"
  | Token.R_bracket ->
      "']'"
  | Token.L_paren ->
      "'('"
  | Token.R_paren ->
      "')'"
  | Token.Comma _ ->
      "','"
  | Token.Dot ->
      "'.'"
  | Token.Paste ->
      "'#'"
  | Token.Colon ->
      "':'"
  | Token.Of_shape ->
      "'::'"
  | Token.Maps_to ->
      "'=>'"
  | Token.Arrow ->
      "'->'"
  | Token.Has_value ->
      "'<<='"
  | Token.Equal ->
      "'='"
  | Token.Hole ->
      "'?'"
  | Token.Eof ->
      "end of file"
  | Token.Trivia _ ->
      "trivia"
  | Token.Error message ->
      Printf.sprintf "error token %S" message

let span_of_parser_error source = function
  | Some { original= Some token; _ } ->
      Token.span token
  | Some { original= None; startp; endp; _ } ->
      let start = Pos.point_of_lexing source startp in
      let stop = Pos.point_of_lexing source endp in
      Pos.make_span ~start ~stop
  | None ->
      Pos.point_span Pos.unknown_point

let message_of_parser_error = function
  | Some { original= Some token; _ } -> (
      match Token.kind token with
      | Token.Eof ->
          "unexpected end of file"
      | _ ->
          Printf.sprintf "unexpected %s" (describe_token token))
  | _ ->
      "unexpected parser error"

let rec loop checkpoint tokens last =
  match checkpoint with
  | I.InputNeeded _ -> (
      match tokens with
      | item :: rest ->
          let checkpoint =
            I.offer checkpoint (item.menhir, item.startp, item.endp)
          in
          loop checkpoint rest (Some item)
      | [] ->
          let pos = Lexing.dummy_pos in
          let checkpoint = I.offer checkpoint (Parser.EOF, pos, pos) in
          loop checkpoint [] last)
  | I.Shifting _ | I.AboutToReduce _ ->
      loop (I.resume checkpoint) tokens last
  | I.Accepted ast ->
      ast
  | I.HandlingError _ ->
      raise (Parse_error last)
  | I.Rejected ->
      raise (Parse_error last)

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
  let source = Token_stream.source stream in
  let ast =
    try loop checkpoint tokens None with
    | Parse_error offending ->
        let span = span_of_parser_error source offending in
        let message = message_of_parser_error offending in
        diagnostics := add_parse_error !diagnostics span message
        ; Ast.empty
    | Failure message ->
        let span = Pos.point_span Pos.unknown_point in
        diagnostics := add_parse_error !diagnostics span message
        ; Ast.empty
  in
  (ast, !diagnostics)
