module Paste_tree = struct
  type t = Leaf of Id.Tag.t | Node of int * t * t
end

type sign = [ `Input | `Output ]
type data = { shape: Ogposet.t; labels: Id.Tag.t array array }

type t = {
  shape: Ogposet.t;
  labels: Id.Tag.t array array;
  tree: sign -> int -> Paste_tree.t;
}

type cell_data = Zero | Boundary of { boundary_in: t; boundary_out: t }
type error = Error.t
type 'a checked = 'a Error.checked

let of_data shape labels tree = { shape; labels; tree }
let shape d = d.shape
let labels d = d.labels
let dim d = Ogposet.dim d.shape
let is_round d = Ogposet.is_round d.shape

let is_cell d =
  match d.tree `Input (dim d) with
  | Paste_tree.Leaf _ ->
      true
  | Paste_tree.Node _ ->
      false

let tree d = d.tree

let cell0 tag =
  let shape = Ogposet.point in
  let labels = [| [| tag |] |] in
  let tree_fn _ _ = Paste_tree.Leaf tag in
  Ok { shape; labels; tree= tree_fn }

let pullback_data d emb =
  let dom = Ogposet.Embedding.dom emb in
  let cod_labels = labels d in
  let map = Ogposet.Embedding.map emb in
  let pulled =
    Array.mapi
      (fun dim level_map ->
        Array.map (fun idx -> cod_labels.(dim).(idx)) level_map)
      map
  in
  { shape= dom; labels= pulled }

let labels_equal a b =
  let len = Array.length a in
  len = Array.length b
  &&
  let rec dim_loop i =
    if i = len then true
    else
      let row_a = a.(i) and row_b = b.(i) in
      let row_len = Array.length row_a in
      row_len = Array.length row_b
      &&
      let rec cell_loop j =
        if j = row_len then true
        else if row_a.(j) = row_b.(j) then cell_loop (j + 1)
        else false
      in
      cell_loop 0 && dim_loop (i + 1)
  in
  dim_loop 0

let parallelism u v =
  let shape_u = shape u and shape_v = shape v in
  let dim_u = Ogposet.dim shape_u and dim_v = Ogposet.dim shape_v in
  if dim_u <> dim_v then Error (Error.make "dimensions do not match")
  else if not (is_round u) then Error (Error.make "first argument is not round")
  else if not (is_round v) then
    Error (Error.make "second argument is not round")
  else
    let bd_u, e_u = Ogposet.boundary_traverse `Both dim_u shape_u in
    let bd_v, e_v = Ogposet.boundary_traverse `Both dim_u shape_v in
    if not (Ogposet.equal bd_u bd_v) then
      Error (Error.make "shapes of boundaries do not match")
    else
      let pb_u = pullback_data u e_u and pb_v = pullback_data v e_v in
      if not (labels_equal pb_u.labels pb_v.labels) then
        Error (Error.make "boundaries do not match")
      else Ok (bd_u, e_u, e_v)

let parallel u v = match parallelism u v with Ok _ -> true | Error _ -> false

let pastability k u v =
  if k < 0 then Error (Error.make "dimension of pasting must be positive")
  else
    let shape_u = shape u and shape_v = shape v in
    let dim_u = Ogposet.dim shape_u and dim_v = Ogposet.dim shape_v in
    let out_n_u, e_u =
      Ogposet.boundary_traverse `Output (min k dim_u) shape_u
    in
    let in_n_v, e_v = Ogposet.boundary_traverse `Input (min k dim_v) shape_v in
    if not (Ogposet.equal out_n_u in_n_v) then
      Error (Error.make "shapes of boundaries do not match")
    else
      let pb_u = pullback_data u e_u and pb_v = pullback_data v e_v in
      if not (labels_equal pb_u.labels pb_v.labels) then
        Error (Error.make "boundaries do not match")
      else Ok (out_n_u, e_u, e_v)

let pastable k u v =
  match pastability k u v with Ok _ -> true | Error _ -> false

let is_normal d = Ogposet.is_normal d.shape

let cellN tag u v =
  match parallelism u v with
  | Error _ as err ->
      err
  | Ok (_, e_u, e_v) ->
      let d = dim u in
      let { Ogposet.tip= bd_uv; inl; inr } = Ogposet.pushout e_u e_v in
      let sizes_bd = Ogposet.sizes bd_uv in
      let inl_map = Ogposet.Embedding.map inl in
      let inr_map = Ogposet.Embedding.map inr in
      let inl_inv = Ogposet.Embedding.inv inl in
      let inr_inv = Ogposet.Embedding.inv inr in
      let inl_map_d = inl_map.(d) in
      let inr_map_d = inr_map.(d) in
      let inl_inv_d = inl_inv.(d) in
      let inr_inv_d = inr_inv.(d) in
      let faces_in =
        Array.init (d + 2) (fun dim ->
            if dim <= d then
              Array.init sizes_bd.(dim) (fun pos ->
                  Ogposet.faces_of `Input bd_uv ~dim ~pos)
            else [| Ogposet.intset_of_array inl_map_d |])
      in
      let faces_out =
        Array.init (d + 2) (fun dim ->
            if dim <= d then
              Array.init sizes_bd.(dim) (fun pos ->
                  Ogposet.faces_of `Output bd_uv ~dim ~pos)
            else [| Ogposet.intset_of_array inr_map_d |])
      in
      let cofaces_in =
        Array.init (d + 2) (fun dim ->
            if dim < d then
              Array.init sizes_bd.(dim) (fun pos ->
                  Ogposet.cofaces_of `Input bd_uv ~dim ~pos)
            else if dim = d then
              Array.init (Array.length inl_inv_d) (fun idx ->
                  if inl_inv_d.(idx) >= 0 then Ogposet.intset_of_array [| 0 |]
                  else Ogposet.intset_empty)
            else [| Ogposet.intset_empty |])
      in
      let cofaces_out =
        Array.init (d + 2) (fun dim ->
            if dim < d then
              Array.init sizes_bd.(dim) (fun pos ->
                  Ogposet.cofaces_of `Output bd_uv ~dim ~pos)
            else if dim = d then
              Array.init (Array.length inr_inv_d) (fun idx ->
                  if inr_inv_d.(idx) >= 0 then Ogposet.intset_of_array [| 0 |]
                  else Ogposet.intset_empty)
            else [| Ogposet.intset_empty |])
      in
      let shape_uv =
        Ogposet.make ~dim:(d + 1) ~faces_in ~faces_out ~cofaces_in ~cofaces_out
      in
      let labels_u = labels u and labels_v = labels v in
      let labels_base_opts =
        Array.init (d + 1) (fun dim -> Array.make sizes_bd.(dim) None)
      in
      Array.iteri
        (fun dim mapping ->
          Array.iteri
            (fun idx target ->
              labels_base_opts.(dim).(target) <- Some labels_u.(dim).(idx))
            mapping)
        inl_map
      ; Array.iteri
          (fun dim mapping ->
            Array.iteri
              (fun idx target ->
                labels_base_opts.(dim).(target) <- Some labels_v.(dim).(idx))
              mapping)
          inr_map
      ; let labels_bd =
          Array.map
            (Array.map (function
              | Some tag_value ->
                  tag_value
              | None ->
                  assert false))
            labels_base_opts
        in
        let labels_uv =
          Array.init (d + 2) (fun dim ->
              if dim <= d then labels_bd.(dim) else [| tag |])
        in
        let tree_u = u.tree and tree_v = v.tree in
        let tree_fn sign k =
          if k < d then tree_u sign k
          else if k = d then
            match sign with `Input -> tree_u sign k | `Output -> tree_v sign k
          else Paste_tree.Leaf tag
        in
        Ok { shape= shape_uv; labels= labels_uv; tree= tree_fn }

let cell tag = function
  | Zero ->
      cell0 tag
  | Boundary { boundary_in; boundary_out } ->
      cellN tag boundary_in boundary_out

let normal u =
  if is_normal u then u
  else
    let shape_norm, emb = Ogposet.normalisation u.shape in
    let pulled = pullback_data u emb in
    of_data shape_norm pulled.labels u.tree

let paste n u v =
  match pastability n u v with
  | Error _ as err ->
      err
  | Ok (_, e_u, e_v) ->
      let { Ogposet.tip= shape_uv; inl; inr } = Ogposet.pushout e_u e_v in
      let sizes_uv = Ogposet.sizes shape_uv in
      let labels_u = labels u and labels_v = labels v in
      let base_labels =
        Array.init (Array.length sizes_uv) (fun dim ->
            Array.make sizes_uv.(dim) None)
      in
      Array.iteri
        (fun dim mapping ->
          Array.iteri
            (fun idx target ->
              base_labels.(dim).(target) <- Some labels_u.(dim).(idx))
            mapping)
        (Ogposet.Embedding.map inl)
      ; Array.iteri
          (fun dim mapping ->
            Array.iteri
              (fun idx target ->
                base_labels.(dim).(target) <- Some labels_v.(dim).(idx))
              mapping)
          (Ogposet.Embedding.map inr)
      ; let labels_uv =
          Array.map
            (Array.map (function
              | Some tag_value ->
                  tag_value
              | None ->
                  assert false))
            base_labels
        in
        let tree_u = u.tree and tree_v = v.tree in
        let tree_fn sign k =
          if k < n then tree_u sign k
          else if k = n then
            match sign with `Input -> tree_u sign k | `Output -> tree_v sign k
          else
            let t_u = tree_u sign k in
            let t_v = tree_v sign k in
            Paste_tree.Node (n, t_u, t_v)
        in
        Ok { shape= shape_uv; labels= labels_uv; tree= tree_fn }

let boundary (sign : sign) k d =
  if k < 0 then Error (Error.make "Negative-dimensional boundary not allowed")
  else
    let _, emb = Ogposet.boundary (sign :> Ogposet.sign) k d.shape in
    let tree_fn s k' = if k' < k then d.tree s k' else d.tree sign k in
    let pulled = pullback_data d emb in
    Ok (of_data pulled.shape pulled.labels tree_fn)

let boundary_normal (sign : sign) k d =
  if k < 0 then Error (Error.make "Negative-dimensional boundary not allowed")
  else
    let shape_norm, emb =
      Ogposet.boundary_traverse (sign :> Ogposet.sign) k d.shape
    in
    let tree_fn s k' = if k' < k then d.tree s k' else d.tree sign k in
    let pulled = pullback_data d emb in
    Ok (of_data shape_norm pulled.labels tree_fn)

let label_set_of d =
  let table = Hashtbl.create 16 in
  Array.iter
    (Array.iter (fun tag ->
         let count =
           match Hashtbl.find_opt table tag with Some c -> c + 1 | None -> 1
         in
         Hashtbl.replace table tag count))
    d.labels
  ; Hashtbl.fold (fun tag count acc -> (tag, count) :: acc) table []
    |> List.sort (fun (a, _) (b, _) -> Id.Tag.compare a b)

let equal u v =
  if u == v then true
  else Ogposet.equal u.shape v.shape && labels_equal u.labels v.labels

let isomorphic u v =
  if equal u v then true
  else
    let shape_u = u.shape and shape_v = v.shape in
    match Ogposet.isomorphism_of shape_u shape_v with
    | Error _ ->
        false
    | Ok iso ->
        let pulled = pullback_data v iso in
        labels_equal u.labels pulled.labels

let isomorphism_of u v =
  let shape_u = u.shape and shape_v = v.shape in
  if u == v then Ok (Ogposet.Embedding.id shape_u)
  else
    match Ogposet.isomorphism_of shape_u shape_v with
    | Error _ as err ->
        err
    | Ok iso ->
        let pulled = pullback_data v iso in
        if labels_equal u.labels pulled.labels then Ok iso
        else Error (Error.make "labels do not match")

let has_local_labels diagram =
  Array.exists
    (Array.exists (function `Local _ -> true | `Global _ -> false))
    diagram.labels
