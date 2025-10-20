type t = { message: string; notes: string list }
type error = t

let make ?(notes = []) message = { message; notes }

let pp fmt { message; notes } =
  let open Format in
  fprintf fmt "%s" message
  ; match notes with
    | [] ->
        ()
    | _ ->
        List.iter (fun note -> fprintf fmt "@.@[<2>note:@ %s@]" note) notes

type 'a checked = ('a, t) result

let base_make = make
let base_pp = pp

module Located = struct
  type phase = [ `Lexer | `Parser | `Driver | `Interpreter | `Other of string ]
  type producer = { phase: phase; module_path: string option }

  module Span = struct
    type t = { start: Lexing.position; stop: Lexing.position }

    let normalize start stop =
      if stop.Lexing.pos_cnum < start.Lexing.pos_cnum then
        { start= stop; stop= start }
      else { start; stop }

    let make start stop = normalize start stop
    let point position = { start= position; stop= position }
  end

  type t = { error: error; span: Span.t; producer: producer }

  let make ?notes producer span message =
    let error = base_make ?notes message in
    { error; span; producer }

  let attach producer span error = { error; span; producer }
  let map_error f located = { located with error= f located.error }
  let to_error { error; _ } = error

  let phase_to_string = function
    | `Lexer ->
        "lexer"
    | `Parser ->
        "parser"
    | `Driver ->
        "driver"
    | `Interpreter ->
        "interpreter"
    | `Other label ->
        label

  let origin_to_string { phase; module_path } =
    match module_path with
    | None ->
        phase_to_string phase
    | Some path ->
        phase_to_string phase ^ ":" ^ path

  let column position = position.Lexing.pos_cnum - position.Lexing.pos_bol + 1

  let span_to_string span =
    let open Span in
    let { start; stop } = span in
    Format.asprintf "%d:%d-%d:%d" start.Lexing.pos_lnum (column start)
      stop.Lexing.pos_lnum (column stop)

  let pp fmt { producer; span; error } =
    let origin = origin_to_string producer in
    let span_text = span_to_string span in
    Format.fprintf fmt "@[<v>[%s @ %s]@,%a@]" origin span_text base_pp error
end

type located = Located.t
