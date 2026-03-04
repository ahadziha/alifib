module TagTable = Hashtbl.Make (struct
  type t = Id.Tag.t

  let equal = Id.Tag.equal

  let hash = function
    | `Local name ->
        Hashtbl.hash (0, Id.Local.to_string name)
    | `Global id ->
        Hashtbl.hash (1, Id.Global.to_int id)
end)

module IntMap = Map.Make (Int)

type 'a checked = 'a Error.checked
type cell_data = Diagram.cell_data
type entry = { dim: int; cell_data: cell_data; image: Diagram.t }

type t = {
  table: entry TagTable.t;
  cellular: bool;
  by_dim: Id.Tag.t list IntMap.t;
}

let make_entry ~dim ~cell_data ~image = { dim; cell_data; image }

let dim_bucket map dim =
  match IntMap.find_opt dim map with Some tags -> tags | None -> []

let init () =
  let table = TagTable.create 1 in
  Ok { table; cellular= true; by_dim= IntMap.empty }

let of_entries entries ~cellular =
  let table = TagTable.create (List.length entries) in
  let by_dim =
    List.fold_left
      (fun acc (tag, dim, cell_data, image) ->
        TagTable.replace table tag (make_entry ~dim ~cell_data ~image)
        ; let existing = dim_bucket acc dim in
          IntMap.add dim (tag :: existing) acc)
      IntMap.empty entries
  in
  { table; cellular; by_dim }

let domain_of_definition m =
  TagTable.fold (fun tag _ acc -> tag :: acc) m.table []

let is_defined_at m tag = TagTable.mem m.table tag

let cell_data m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.cell_data
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

let domain_in_dim m dim =
  match IntMap.find_opt dim m.by_dim with
  | Some tags ->
      List.rev tags
  | None ->
      []

let dimensions m = IntMap.bindings m.by_dim |> List.map fst

let domain_by_dim m =
  IntMap.bindings m.by_dim |> List.map (fun (dim, tags) -> (dim, List.rev tags))

let has_local_labels m =
  TagTable.fold
    (fun _ { image; _ } acc -> acc || Diagram.has_local_labels image)
    m.table false

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
              Ok (image_exn tag)
          | Diagram.Paste_tree.Node (k, t1, t2) -> (
              match f_tree t1 with
              | Error _ as err ->
                  err
              | Ok d1 -> (
                  match f_tree t2 with
                  | Error _ as err ->
                      err
                  | Ok d2 -> (
                      match Diagram.paste k d1 d2 with
                      | Ok d ->
                          Ok d
                      | Error err ->
                          Error err)))
        in
        f_tree root_tree

let extend f ~tag ~dim ~cell_data ~image =
  if is_defined_at f tag then Error (Error.make "already defined")
  else if Diagram.dim image <> dim then
    Error (Error.make "dimensions do not match")
  else if not (Diagram.is_round image) then
    Error (Error.make "image is not round")
  else if dim = 0 then
    match cell_data with
    | Diagram.Zero ->
        let table = TagTable.copy f.table in
        TagTable.replace table tag (make_entry ~dim ~cell_data ~image)
        ; let by_dim =
            let existing = dim_bucket f.by_dim dim in
            IntMap.add dim (tag :: existing) f.by_dim
          in
          Ok { table; cellular= f.cellular; by_dim }
    | Diagram.Boundary _ ->
        assert false
  else
    match cell_data with
    | Diagram.Zero ->
        assert false
    | Diagram.Boundary { boundary_in; boundary_out } -> (
        match apply f boundary_in with
        | Error _ as err ->
            err
        | Ok mapped_in -> (
            match apply f boundary_out with
            | Error _ as err ->
                err
            | Ok mapped_out -> (
                let boundary_idx = dim - 1 in
                match Diagram.boundary_normal `Input boundary_idx image with
                | Error _ as err ->
                    err
                | Ok expected_input -> (
                    let mapped_input = Diagram.normal mapped_in in
                    if not (Diagram.equal mapped_input expected_input) then
                      Error (Error.make "input boundaries do not match")
                    else
                      match
                        Diagram.boundary_normal `Output boundary_idx image
                      with
                      | Error _ as err ->
                          err
                      | Ok expected_output ->
                          let mapped_output = Diagram.normal mapped_out in
                          if not (Diagram.equal mapped_output expected_output)
                          then
                            Error (Error.make "output boundaries do not match")
                          else
                            let table = TagTable.copy f.table in
                            TagTable.replace table tag
                              (make_entry ~dim ~cell_data ~image)
                            ; let cellular =
                                f.cellular && Diagram.is_cell image
                              in
                              let by_dim =
                                let existing = dim_bucket f.by_dim dim in
                                IntMap.add dim (tag :: existing) f.by_dim
                              in
                              Ok { table; cellular; by_dim }))))

let compose g f =
  let initial =
    {
      table= TagTable.create (max 1 (TagTable.length f.table));
      cellular= true;
      by_dim= IntMap.empty;
    }
  in
  let add_tag dim tag acc =
    match image f tag with
    | Error _ ->
        acc
    | Ok image_f_tag -> (
        match apply g image_f_tag with
        | Error _ ->
            acc
        | Ok image_gf -> (
            match cell_data f tag with
            | Error _ ->
                acc
            | Ok cell_data_f ->
                TagTable.replace acc.table tag
                  (make_entry ~dim ~cell_data:cell_data_f ~image:image_gf)
                ; let by_dim =
                    IntMap.update dim
                      (function
                        | None -> Some [ tag ] | Some tags -> Some (tag :: tags))
                      acc.by_dim
                  in
                  let cellular = acc.cellular && Diagram.is_cell image_gf in
                  { acc with by_dim; cellular }))
  in
  List.fold_left
    (fun acc (dim, tags) ->
      List.fold_left (fun acc tag -> add_tag dim tag acc) acc tags)
    initial (domain_by_dim f)
