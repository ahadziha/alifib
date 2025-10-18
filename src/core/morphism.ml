module TagTable = Hashtbl.Make (struct
  type t = Id.Tag.t

  let equal = Id.Tag.equal

  let hash = function
    | `Local name ->
        Hashtbl.hash (0, Id.Local.to_string name)
    | `Global id ->
        Hashtbl.hash (1, Id.Global.to_int id)
end)

type 'a checked = 'a Error.checked
type cell_data = { boundary_in: Diagram.t; boundary_out: Diagram.t }
type entry = { dim: int; boundaries: cell_data; image: Diagram.t }
type t = { table: entry TagTable.t; cellular: bool }

let init () =
  let table = TagTable.create 1 in
  Ok { table; cellular= true }

let[@warning "-32"] of_list entries =
  (* Private helper: assumes entries already validated. *)
  let capacity = max 1 (List.length entries) in
  let table = TagTable.create capacity in
  let cellular = ref true in
  List.iter
    (fun (tag, dim, boundaries, image) ->
      TagTable.replace table tag { dim; boundaries; image }
      ; if !cellular then ()
        else if not (Diagram.is_cell image) then cellular := false)
    entries
  ; { table; cellular= !cellular }

let domain_of_definition m =
  TagTable.fold (fun tag _ acc -> tag :: acc) m.table []

let is_defined_at m tag = TagTable.mem m.table tag

let cell_data m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.boundaries
  | None ->
      Error (Error.make "not in the domain of definition")

let dim m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.dim
  | None ->
      Error (Error.make "not in the domain of definition")

let image m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.image
  | None ->
      Error (Error.make "not in the domain of definition")

let is_cellular m = m.cellular

let apply f diagram =
  let image_exn tag =
    match image f tag with Ok d -> d | Error _ -> assert false
  in
  let n = Diagram.dim diagram in
  let root_tree = Diagram.tree diagram `Input n in
  let rec find_undefined = function
    | Diagram.Paste_tree.Leaf tag ->
        if is_defined_at f tag then None else Some tag
    | Diagram.Paste_tree.Node (_, t1, t2) -> (
        match find_undefined t1 with
        | Some _ as missing ->
            missing
        | None ->
            find_undefined t2)
  in
  match find_undefined root_tree with
  | Some tag ->
      let note = Format.asprintf "tag: %a" Id.Tag.pp tag in
      Error
        (Error.make ~notes:[ note ]
           "diagram value outside of domain of definition")
  | None ->
      (* Simplified application if f is cellular *)
      if is_cellular f then
        let shape_d = Diagram.shape diagram in
        let original_labels = Diagram.labels diagram in
        let tag_cache = Hashtbl.create 16 in
        let top_label tag =
          match Hashtbl.find_opt tag_cache tag with
          | Some mapped ->
              mapped
          | None ->
              let cell_diagram = image_exn tag in
              let top_dim = Diagram.dim cell_diagram in
              let labels = Diagram.labels cell_diagram in
              let top_labels = labels.(top_dim) in
              assert (Array.length top_labels = 1)
              ; let mapped = top_labels.(0) in
                Hashtbl.add tag_cache tag mapped
                ; mapped
        in
        let new_labels = Array.map (Array.map top_label) original_labels in
        let rec map_tree = function
          | Diagram.Paste_tree.Leaf tag ->
              Diagram.Paste_tree.Leaf (top_label tag)
          | Diagram.Paste_tree.Node (k, t1, t2) ->
              Diagram.Paste_tree.Node (k, map_tree t1, map_tree t2)
        in
        let tree_fn sign k = map_tree (Diagram.tree diagram sign k) in
        let open Diagram in
        Ok { shape= shape_d; labels= new_labels; tree= tree_fn }
      else
        (* Fall back to rebuilding via the paste tree *)
        let rec f_tree = function
          | Diagram.Paste_tree.Leaf tag ->
              image_exn tag
          | Diagram.Paste_tree.Node (k, t1, t2) -> (
              let d1 = f_tree t1 in
              let d2 = f_tree t2 in
              match Diagram.paste k d1 d2 with
              | Ok d ->
                  d
              | Error _ ->
                  assert false)
        in
        Ok (f_tree root_tree)

let extend f ~tag ~dim ~boundary_in ~boundary_out ~image =
  if is_defined_at f tag then Error (Error.make "already defined")
  else if Diagram.dim image <> dim then
    Error (Error.make "dimensions do not match")
  else if not (Diagram.is_round image) then
    Error (Error.make "image is not round")
  else
    match apply f boundary_in with
    | Error _ as err ->
        err
    | Ok mapped_in -> (
        match apply f boundary_out with
        | Error _ as err ->
            err
        | Ok mapped_out ->
            let boundary_idx = dim - 1 in
            let expected_input =
              Diagram.boundary_normal `Input boundary_idx image
            in
            let mapped_input = Diagram.normal mapped_in in
            if not (Diagram.equal mapped_input expected_input) then
              Error (Error.make "input boundaries do not match")
            else
              let expected_output =
                Diagram.boundary_normal `Output boundary_idx image
              in
              let mapped_output = Diagram.normal mapped_out in
              if not (Diagram.equal mapped_output expected_output) then
                Error (Error.make "output boundaries do not match")
              else
                let table = TagTable.copy f.table in
                TagTable.replace table tag
                  { dim; boundaries= { boundary_in; boundary_out }; image }
                ; let cellular = f.cellular && Diagram.is_cell image in
                  Ok { table; cellular })
