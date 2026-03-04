open Positions

type t = {
  source: Source.t;
  mutable text: string;
  mutable tokens: Token.t array;
  mutable diagnostics: Diagnostics.report;
}

type cursor = { stream: t; mutable index: int }

let ensure_span_source stream span =
  if not (Source.equal span.start.source stream.source) then
    invalid_arg "Token_stream.apply_edit: span source differs from stream"

let relex stream =
  let tokens, diagnostics = Lexer.run ~source:stream.source stream.text in
  stream.tokens <- Array.of_list tokens
  ; stream.diagnostics <- diagnostics

let create ~source text =
  let tokens, diagnostics = Lexer.run ~source text in
  { source; text; tokens= Array.of_list tokens; diagnostics }

let source stream = stream.source
let text stream = stream.text
let tokens stream = stream.tokens
let diagnostics stream = stream.diagnostics
let reload stream = relex stream

let replace_substring text ~start ~stop replacement =
  let len = String.length text in
  if start < 0 || stop < start || stop > len then
    invalid_arg "Token_stream.apply_edit: invalid span bounds"
  else
    let prefix = String.sub text 0 start in
    let suffix = String.sub text stop (len - stop) in
    prefix ^ replacement ^ suffix

let apply_edit stream ~span ~replacement =
  ensure_span_source stream span
  ; let start_offset = span.start.offset in
    let stop_offset = span.stop.offset in
    stream.text <-
      replace_substring stream.text ~start:start_offset ~stop:stop_offset
        replacement
    ; relex stream

let cursor stream = { stream; index= 0 }

let next cursor =
  if cursor.index < Array.length cursor.stream.tokens then (
    let token = cursor.stream.tokens.(cursor.index) in
    cursor.index <- cursor.index + 1
    ; Some token)
  else None

let peek cursor =
  if cursor.index < Array.length cursor.stream.tokens then
    Some cursor.stream.tokens.(cursor.index)
  else None

let reset cursor = cursor.index <- 0
