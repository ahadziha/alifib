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

let make ?(notes = []) ?(details = []) ?code severity producer span headline =
  let payload = Error.Located.make ~notes producer span headline in
  { severity; payload; message = { headline; details }; code }

let of_error ?code ?(details = []) severity payload =
  {
    severity;
    payload;
    message = { headline = payload.error.message; details };
    code;
  }

let with_details details t =
  { t with message = { t.message with details } }

let add_detail detail t =
  with_details (t.message.details @ [ detail ]) t

let add_note note t =
  let error = t.payload.error in
  let updated_notes =
    if List.exists (String.equal note) error.notes then
      error.notes
    else
      error.notes @ [ note ]
  in
  let payload = { t.payload with error = { error with notes = updated_notes } } in
  { t with payload }

let map_error f t =
  let old_error = t.payload.error in
  let new_error = f old_error in
  let payload = { t.payload with error = new_error } in
  let message =
    if String.equal t.message.headline old_error.message then
      { t.message with headline = new_error.message }
    else
      t.message
  in
  { t with payload; message }

let span t = t.payload.span
let producer t = t.payload.producer
let to_error t = t.payload.error
let to_located_error t = t.payload

let pp_severity fmt = function
  | `Error ->
      Format.fprintf fmt "error"
  | `Warning ->
      Format.fprintf fmt "warning"
  | `Info ->
      Format.fprintf fmt "info"

let pp_code fmt = function
  | None ->
      ()
  | Some code ->
      Format.fprintf fmt " [%s]" code

let pp_message fmt { headline; details } =
  Format.fprintf fmt "%s" headline ;
  List.iter (fun detail -> Format.fprintf fmt "@,@[<2>%s@]" detail) details

let phase_to_string : phase -> string = function
  | `Lexer -> "lexer"
  | `Parser -> "parser"
  | `Driver -> "driver"
  | `Interpreter -> "interpreter"
  | `Other label -> label

let producer_to_string { Error.Located.phase; module_path } =
  match module_path with
  | None -> phase_to_string phase
  | Some path -> phase_to_string phase ^ ":" ^ path

let pp fmt ({ severity; payload; message; code } as diagnostic) =
  let open Format in
  let span_value = span diagnostic in
  fprintf fmt "@[<v>%a%a: %a@,@[<2>origin:@ %s@]@,@[<2>span:@ %a@]"
    pp_severity severity
    pp_code code
    pp_message message
    (producer_to_string payload.producer)
    pp_span span_value ;
  List.iter (fun note -> fprintf fmt "@,@[<2>note:@ %s@]" note) payload.error.notes ;
  fprintf fmt "@]"

type report = t list

module Report = struct
  type t = report

  let empty = []

  let add diagnostic report = diagnostic :: report

  let append left right = left @ right

  let pp fmt report =
    let open Format in
    match report with
    | [] ->
        fprintf fmt "no diagnostics"
    | _ ->
        fprintf fmt "@[<v>%a@]"
          (pp_print_list ~pp_sep:(fun fmt () -> fprintf fmt "@,@,") pp)
          (List.rev report)
end
