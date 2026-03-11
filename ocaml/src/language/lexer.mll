{
open Token

module Pos = Positions
module D = Diagnostics

type env = {
  source : Pos.Source.t;
  diagnostics : D.report ref;
  mutable pending : Token.t list;
  mutable newline_run : int;
}

let producer =
  { Error.Located.phase = `Lexer; module_path = Some "language.lexer" }

let span_of_lexeme env lexbuf =
  Pos.span_of_lexing
    env.source
    (Lexing.lexeme_start_p lexbuf)
    (Lexing.lexeme_end_p lexbuf)

let make_token env kind lexbuf = Token.make kind (span_of_lexeme env lexbuf)

let add_diagnostic env span message =
  let diagnostic = D.make `Error producer span message in
  env.diagnostics := diagnostic :: !(env.diagnostics)

let record_newlines lexbuf lexeme =
  String.iter
    (function
      | '\n' -> Lexing.new_line lexbuf
      | _ -> ())
    lexeme

let enqueue env token = env.pending <- token :: env.pending

let reset_newlines env = env.newline_run <- 0
let bump_newlines env = env.newline_run <- env.newline_run + 1

let make_error env span message =
  add_diagnostic env span message;
  Token.make (Token.Error message) span
}

rule token env = parse
  | [' ' '\t' '\012' '\013']+ {
      let span = span_of_lexeme env lexbuf in
      Token.make (Token.Trivia (Token.Whitespace (Lexing.lexeme lexbuf))) span
    }
  | "\n" | "\r\n" {
      let lexeme = Lexing.lexeme lexbuf in
      record_newlines lexbuf lexeme;
      bump_newlines env;
      let span = span_of_lexeme env lexbuf in
      let newline_token = Token.make (Token.Trivia (Token.Newline lexeme)) span in
      if env.newline_run >= 2 then (
        let comma = Token.make (Token.Comma `From_newline) span in
        enqueue env comma;
        reset_newlines env
      );
      newline_token
    }
  | "(*" {
      reset_newlines env;
      let start_pos = Lexing.lexeme_start_p lexbuf in
      let buffer = Buffer.create 32 in
      comment env buffer start_pos 0 lexbuf
    }
  | "@" {
      reset_newlines env;
      make_token env At lexbuf
    }
  | "<<=" {
      reset_newlines env;
      make_token env Has_value lexbuf
    }
  | "::" {
      reset_newlines env;
      make_token env Of_shape lexbuf
    }
  | "=>" {
      reset_newlines env;
      make_token env Maps_to lexbuf
    }
  | "->" {
      reset_newlines env;
      make_token env Arrow lexbuf
    }
  | ":" {
      reset_newlines env;
      make_token env Colon lexbuf
    }
  | "," {
      reset_newlines env;
      make_token env (Comma `Explicit) lexbuf
    }
  | ("#0" ['0'-'9']+) {
      reset_newlines env;
      let span = span_of_lexeme env lexbuf in
      make_error env span "numbers following '#' must not have leading zeros"
    }
  | ("#0" | "#" ['1'-'9']['0'-'9']*) as lexeme {
      reset_newlines env;
      let start_point =
        Pos.point_of_lexing env.source (Lexing.lexeme_start_p lexbuf)
      in
      let after_hash = Pos.advance start_point "#" in
      let end_point =
        Pos.point_of_lexing env.source (Lexing.lexeme_end_p lexbuf)
      in
      let hash_span = Pos.make_span ~start:start_point ~stop:after_hash in
      let digits = String.sub lexeme 1 (String.length lexeme - 1) in
      let nat_span = Pos.make_span ~start:after_hash ~stop:end_point in
      let nat_token = Token.make (Nat digits) nat_span in
      enqueue env nat_token;
      Token.make Paste hash_span
    }
  | "#" {
      reset_newlines env;
      let span = span_of_lexeme env lexbuf in
      make_error env span "expected digits immediately after '#'"
    }
  | "{" {
      reset_newlines env;
      make_token env L_brace lexbuf
    }
  | "}" {
      reset_newlines env;
      make_token env R_brace lexbuf
    }
  | "[" {
      reset_newlines env;
      make_token env L_bracket lexbuf
    }
  | "]" {
      reset_newlines env;
      make_token env R_bracket lexbuf
    }
  | "(" {
      reset_newlines env;
      make_token env L_paren lexbuf
    }
  | ")" {
      reset_newlines env;
      make_token env R_paren lexbuf
    }
  | "." {
      reset_newlines env;
      make_token env Dot lexbuf
    }
  | "=" {
      reset_newlines env;
      make_token env Equal lexbuf
    }
  | "?" {
      reset_newlines env;
      make_token env Hole lexbuf
    }
  | ['A'-'Z' 'a'-'z' '0'-'9' '_']['A'-'Z' 'a'-'z' '0'-'9' '_' '\'']* as ident {
      reset_newlines env;
      let kind =
        match keyword_of_string ident with
        | Some kw -> Keyword kw
        | None -> Identifier ident
      in
      make_token env kind lexbuf
    }
  | eof {
      reset_newlines env;
      let pos =
        Pos.point_of_lexing env.source (Lexing.lexeme_end_p lexbuf)
      in
      let span = Pos.make_span ~start:pos ~stop:pos in
      Token.make Eof span
    }
  | _ {
      reset_newlines env;
      let span = span_of_lexeme env lexbuf in
      let lexeme = Lexing.lexeme lexbuf in
      let message = Printf.sprintf "unexpected character %S" lexeme in
      add_diagnostic env span message;
      Token.make (Token.Error message) span
    }
(* TODO: Do something nicer so we dont have to repeat all the arguments to `comment`
  everywhere and so we don't have the magic `0` depth value at the call site *)
and comment env buffer start_pos depth = parse
  | "(*" {
      Buffer.add_string buffer (Lexing.lexeme lexbuf);
      comment env buffer start_pos (depth + 1) lexbuf
    }
  | "*)" {
      if depth = 0 then
        let span =
          Pos.span_of_lexing env.source start_pos (Lexing.lexeme_end_p lexbuf)
        in
        Token.make (Token.Trivia (Token.Comment (Buffer.contents buffer))) span
      else (
        Buffer.add_string buffer (Lexing.lexeme lexbuf);
        comment env buffer start_pos (depth - 1) lexbuf
      )
    }
  | "\r\n" | "\n" {
      Buffer.add_string buffer (Lexing.lexeme lexbuf);
      Lexing.new_line lexbuf;
      comment env buffer start_pos depth lexbuf
    }
  | eof {
      let span =
        Pos.span_of_lexing env.source start_pos (Lexing.lexeme_end_p lexbuf)
      in
      add_diagnostic env span "unterminated comment";
      Token.make (Token.Error "unterminated comment") span
    }
  | _ {
      Buffer.add_string buffer (Lexing.lexeme lexbuf);
      comment env buffer start_pos depth lexbuf
    }

{
let next env lexbuf =
  match env.pending with
  | token :: rest ->
      env.pending <- rest;
      token
  | [] ->
      token env lexbuf

let run ~source text =
  let diagnostics = ref [] in
  let env =
    { source; diagnostics; pending = []; newline_run = 0 }
  in
  let lexbuf = Lexing.from_string text in
  let initial_pos = lexbuf.Lexing.lex_curr_p in
  lexbuf.Lexing.lex_curr_p <-
    { initial_pos with pos_fname = Pos.Source.to_string source } ;
  let rec collect acc =
    let token = next env lexbuf in
    match kind token with
    | Eof ->
        List.rev (token :: acc)
    | _ ->
        collect (token :: acc)
  in
  let tokens = collect [] in
  let report = List.rev !(env.diagnostics) in
  tokens, report
}
