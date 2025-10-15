type t = {
  shape: Ogposet.t;
  labels: Id.tag array array;
}

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

let shape d = d.shape
let labels d = d.labels
let dim d = Ogposet.dim d.shape
let is_round d = Ogposet.is_round d.shape

let cell0 tag =
  let shape = Ogposet.point in
  let labels = [| [| tag |] |] in
  Ok { shape; labels }
