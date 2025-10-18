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
type entry = { cell: Diagram.t; image: Diagram.t }
type t = { table: entry TagTable.t }

let[@warning "-32"] of_list entries =
  (* Private helper: assumes entries already validated. *)
  let capacity = max 1 (List.length entries) in
  let table = TagTable.create capacity in
  List.iter
    (fun (tag, cell, image) -> TagTable.replace table tag { cell; image })
    entries
  ; { table }

let domain_of_definition m =
  TagTable.fold (fun tag _ acc -> tag :: acc) m.table []

let is_defined_at m tag = TagTable.mem m.table tag

let cell m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.cell
  | None ->
      Error (Error.make "not in the domain of definition")

let image m tag =
  match TagTable.find_opt m.table tag with
  | Some entry ->
      Ok entry.image
  | None ->
      Error (Error.make "not in the domain of definition")

let apply f diagram =
  let missing_tag =
    diagram |> Diagram.label_set_of
    |> List.find_opt (fun (tag, _) -> not (is_defined_at f tag))
  in
  match missing_tag with
  | Some (tag, _) ->
      let note = Format.asprintf "tag: %a" Id.Tag.pp tag in
      Error
        (Error.make ~notes:[ note ]
           "diagram value outside of domain of definition")
  | None ->
      let rec f_tree = function
        | Diagram.Paste_tree.Leaf tag -> (
            match image f tag with Ok d -> d | Error _ -> assert false)
        | Diagram.Paste_tree.Node (k, t1, t2) -> (
            let d1 = f_tree t1 in
            let d2 = f_tree t2 in
            match Diagram.paste k d1 d2 with
            | Ok d ->
                d
            | Error _ ->
                assert false)
      in
      let n = Diagram.dim diagram in
      let tree = Diagram.tree diagram `Input n in
      Ok (f_tree tree)
