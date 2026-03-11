module Source = struct
  type t = string

  let of_path path = path

  let virtual_ name = Printf.sprintf "<%s>" name

  let unknown = "<unknown>"

  let to_string id = id
  let equal = String.equal
  let compare = String.compare
  let hash = Hashtbl.hash
  let pp fmt id = Format.fprintf fmt "%s" id
end

type point = {
  source : Source.t;
  offset : int;
  line : int;
  column : int;
  bol_offset : int;
}

let validate_point ~offset ~line ~column ~bol_offset =
  if offset < 0 then invalid_arg "Positions.make_point: negative offset"
  else if line <= 0 then invalid_arg "Positions.make_point: line must be positive"
  else if column <= 0 then invalid_arg "Positions.make_point: column must be positive"
  else if bol_offset < 0 then invalid_arg "Positions.make_point: negative bol_offset"

let make_point ~source ~offset ~line ~column ~bol_offset =
  validate_point ~offset ~line ~column ~bol_offset ;
  { source; offset; line; column; bol_offset }

let unknown_point =
  make_point
    ~source:Source.unknown
    ~offset:0
    ~line:1
    ~column:1
    ~bol_offset:0

let compare_point a b =
  let source_cmp = Source.compare a.source b.source in
  if source_cmp <> 0 then source_cmp else compare a.offset b.offset

let pp_point fmt { source; line; column; offset; _ } =
  Format.fprintf fmt "%s:%d:%d (offset %d)" (Source.to_string source) line column offset

let advance start text =
  let len = String.length text in
  let rec loop idx line column bol_offset offset =
    if idx = len then
      { start with line; column; bol_offset; offset }
    else
      let ch = text.[idx] in
      let offset = offset + 1 in
      if Char.equal ch '\n' then
        loop (idx + 1) (line + 1) 1 offset offset
      else
        loop (idx + 1) line (column + 1) bol_offset offset
  in
  loop 0 start.line start.column start.bol_offset start.offset

let with_source point source =
  if Source.equal point.source source then
    point
  else
    { point with source }

type span = { start : point; stop : point }

let ensure_same_source where lhs rhs =
  if not (Source.equal lhs.source rhs.source) then
    invalid_arg ("Positions." ^ where ^ ": points from different sources")

let normalize_span start stop =
  if compare_point start stop <= 0 then
    start, stop
  else
    stop, start

let make_span ~start ~stop =
  ensure_same_source "make_span" start stop ;
  let start, stop = normalize_span start stop in
  { start; stop }

let point_span point = { start = point; stop = point }

let merge a b =
  ensure_same_source "merge" a.start b.start ;
  ensure_same_source "merge" a.stop b.stop ;
  let start = if compare_point a.start b.start <= 0 then a.start else b.start in
  let stop = if compare_point a.stop b.stop >= 0 then a.stop else b.stop in
  { start; stop }

let contains span point =
  ensure_same_source "contains" span.start point ;
  span.start.offset <= point.offset && point.offset <= span.stop.offset

let length span = span.stop.offset - span.start.offset

let is_point span = length span = 0

let between left right =
  if Source.equal left.source right.source then
    Some (make_span ~start:left ~stop:right)
  else
    None

let pp_span fmt span =
  if is_point span then
    Format.fprintf fmt "%a" pp_point span.start
  else
    Format.fprintf fmt "%a-%a" pp_point span.start pp_point span.stop

let point_of_lexing source position =
  let column = position.Lexing.pos_cnum - position.Lexing.pos_bol + 1 in
  make_point
    ~source
    ~offset:position.Lexing.pos_cnum
    ~line:position.Lexing.pos_lnum
    ~column
    ~bol_offset:position.Lexing.pos_bol

let to_lexing point =
  {
    Lexing.pos_fname = Source.to_string point.source;
    pos_lnum = point.line;
    pos_cnum = point.offset;
    pos_bol = point.bol_offset;
  }

let span_of_lexing source start stop =
  make_span
    ~start:(point_of_lexing source start)
    ~stop:(point_of_lexing source stop)
