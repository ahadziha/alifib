type t = int

let counter = ref 0

let fresh () =
  let i = !counter in
  incr counter ; i

let equal = Int.equal
let compare = Int.compare
let hash = Hashtbl.hash
let to_int x = x
let pp fmt x = Format.fprintf fmt "#%d" x

type error = {
  message: string;
  notes: string list;
}

let error ?(notes = []) message = { message; notes }

let pp_error fmt { message; notes } =
  let open Format in
  fprintf fmt "%s" message;
  (match notes with
  | [] -> ()
  | _ ->
      List.iter (fun note -> fprintf fmt "@.@[<2>note:@ %s@]" note) notes)

type 'a checked = ('a, error) result

module Name = struct
  type simple = string
  type t = string

  let reserved = [ "include"; "attach"; "Type"; "along" ]

  let is_valid_char = function
    | 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '_' ->
        true
    | _ ->
        false

  let check_simple s =
    if String.length s = 0 then
      Error (error "name must not be empty")
    else if List.exists (String.equal s) reserved then
      Error (error ("name is reserved: " ^ s))
    else if String.for_all is_valid_char s then Ok s
    else
      Error (error "name contains invalid characters")

  let simple s = check_simple s
  let simple_to_string s = s
  let string_is_empty s = String.length s = 0

  let check_name s =
    if String.length s = 0 then Error (error "name must not be empty")
    else
      let parts = String.split_on_char '.' s in
      if List.exists string_is_empty parts then
        Error (error "name must not contain empty segments")
      else
        let rec validate acc = function
          | [] ->
              Ok (List.rev acc)
          | part :: rest -> (
              match check_simple part with
              | Ok p ->
                  validate (p :: acc) rest
              | Error _ as e ->
                  e)
        in
        match validate [] parts with
        | Ok _ -> Ok s
        | Error err -> Error err

  let make s = check_name s
  let to_string n = n
end

type tag = [ `Local of Name.t | `Global of t ]

let tag_equal a b =
  match (a, b) with
  | `Local x, `Local y ->
      String.equal x y
  | `Global x, `Global y ->
      equal x y
  | (`Local _ | `Global _), _ ->
      false

let tag_compare a b =
  match (a, b) with
  | `Local x, `Local y ->
      String.compare x y
  | `Global x, `Global y ->
      compare x y
  | `Local _, `Global _ ->
      -1
  | `Global _, `Local _ ->
      1

let tag_pp fmt = function
  | `Local name ->
      Format.fprintf fmt "%s" (Name.to_string name)
  | `Global id ->
      pp fmt id
