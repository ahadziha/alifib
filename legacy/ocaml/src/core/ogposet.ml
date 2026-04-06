module IntSet = Set.Make (Int)

type intset = IntSet.t
type sign = [ `Input | `Output | `Both ]
type grade = IntSet.t array
type adjacency = grade array

type t = {
  dim: int; (* stored for efficiency *)
  faces_in: adjacency;
  faces_out: adjacency;
  cofaces_in: adjacency;
  cofaces_out: adjacency;
  normal: bool;
}

type poset = t
type error = Error.t
type 'a checked = 'a Error.checked

let dim x = x.dim
let sizes x = Array.init (x.dim + 1) (fun d -> Array.length x.faces_in.(d))

let make ~dim ~faces_in ~faces_out ~cofaces_in ~cofaces_out =
  { dim; faces_in; faces_out; cofaces_in; cofaces_out; normal= false }

let is_normal g = g.normal
let intset_empty = IntSet.empty
let intset_add x s = IntSet.add x s

let intset_of_list lst =
  List.fold_left (fun acc x -> IntSet.add x acc) IntSet.empty lst

let intset_of_array arr =
  Array.fold_left (fun acc x -> IntSet.add x acc) IntSet.empty arr

let intset_elements set = IntSet.elements set

let empty =
  {
    dim= -1;
    faces_in= [||];
    faces_out= [||];
    cofaces_in= [||];
    cofaces_out= [||];
    normal= true;
  }

let point =
  let make_level () = Array.make 1 IntSet.empty in
  {
    dim= 0;
    faces_in= [| make_level () |];
    faces_out= [| make_level () |];
    cofaces_in= [| make_level () |];
    cofaces_out= [| make_level () |];
    normal= true;
  }

let faces_of sign x ~dim ~pos =
  match sign with
  | `Input ->
      x.faces_in.(dim).(pos)
  | `Output ->
      x.faces_out.(dim).(pos)
  | `Both ->
      IntSet.union x.faces_in.(dim).(pos) x.faces_out.(dim).(pos)

let cofaces_of sign x ~dim ~pos =
  match sign with
  | `Input ->
      x.cofaces_in.(dim).(pos)
  | `Output ->
      x.cofaces_out.(dim).(pos)
  | `Both ->
      IntSet.union x.cofaces_in.(dim).(pos) x.cofaces_out.(dim).(pos)

let equal (a : t) (b : t) : bool =
  if a == b then true
  else
    let dims_equal = Array.length a.faces_in = Array.length b.faces_in in
    if not dims_equal then false
    else
      let rec eq_level arr1 arr2 idx =
        let len1 = Array.length arr1 in
        let len2 = Array.length arr2 in
        if len1 <> len2 then false
        else if idx = len1 then true
        else IntSet.equal arr1.(idx) arr2.(idx) && eq_level arr1 arr2 (idx + 1)
      in
      let rec eq_family fam1 fam2 d =
        let len1 = Array.length fam1 in
        let len2 = Array.length fam2 in
        if len1 <> len2 then false
        else if d = len1 then true
        else eq_level fam1.(d) fam2.(d) 0 && eq_family fam1 fam2 (d + 1)
      in
      eq_family a.faces_in b.faces_in 0 && eq_family a.faces_out b.faces_out 0

module Embedding = struct
  type t = {
    dom: poset;
    cod: poset;
    map: int array array;
    inv: int array array;
  }

  let dom e = e.dom
  let cod e = e.cod
  let map e = e.map
  let inv e = e.inv
  let make ~dom ~cod ~map ~inv = { dom; cod; map; inv }

  let empty cod =
    let map = [||] in
    let inv = Array.map (fun n -> Array.make n (-1)) (sizes cod) in
    make ~dom:empty ~cod ~map ~inv

  let id x =
    let sz = sizes x in
    let map = Array.mapi (fun _ n -> Array.init n (fun i -> i)) sz in
    let inv = Array.map Array.copy map in
    make ~dom:x ~cod:x ~map ~inv

  let compose f g =
    let map =
      Array.mapi
        (fun d arrf ->
          Array.init (Array.length arrf) (fun i ->
              let mid = arrf.(i) in
              g.map.(d).(mid)))
        f.map
    in
    let inv =
      Array.init (Array.length g.inv) (fun d ->
          let cod_level = g.inv.(d) in
          Array.init (Array.length cod_level) (fun idx ->
              let mid = cod_level.(idx) in
              if mid < 0 then -1
              else if d >= Array.length f.inv then -1
              else
                let finv_level = f.inv.(d) in
                if mid >= Array.length finv_level then -1 else finv_level.(mid)))
    in
    make ~dom:f.dom ~cod:g.cod ~map ~inv
end

(* --- Utility helpers -------------------------------------------------- *)

let set_map f s = IntSet.fold (fun x acc -> IntSet.add (f x) acc) s IntSet.empty

let set_filter_map f s =
  IntSet.fold
    (fun x acc -> match f x with None -> acc | Some y -> IntSet.add y acc)
    s IntSet.empty

let remap_adjacency ~(levels : int) ~(forward : int array array)
    ~(inv_dom : int array array) ~(shift : int) (adj : adjacency) : adjacency =
  let empty = IntSet.empty in
  Array.init levels (fun j ->
      let nj = Array.length forward.(j) in
      if (shift = -1 && j = 0) || (shift = 1 && j = levels - 1) then
        Array.make nj empty
      else
        Array.init nj (fun i ->
            let old = forward.(j).(i) in
            let target_dim = j + shift in
            if shift = -1 then
              set_map (fun x -> inv_dom.(target_dim).(x)) adj.(j).(old)
            else
              set_filter_map
                (fun x ->
                  let y = inv_dom.(target_dim).(x) in
                  if y < 0 then None else Some y)
                adj.(j).(old)))

let extremal (s : sign) (k : int) (g : t) : intset =
  let n = Array.length g.faces_in.(k) in
  match s with
  | `Input ->
      let acc = ref IntSet.empty in
      for i = 0 to n - 1 do
        if IntSet.is_empty (cofaces_of `Output g ~dim:k ~pos:i) then
          acc := IntSet.add i !acc
      done
      ; !acc
  | `Output ->
      let acc = ref IntSet.empty in
      for i = 0 to n - 1 do
        if IntSet.is_empty (cofaces_of `Input g ~dim:k ~pos:i) then
          acc := IntSet.add i !acc
      done
      ; !acc
  | `Both ->
      let acc = ref IntSet.empty in
      for i = 0 to n - 1 do
        if
          IntSet.is_empty (cofaces_of `Input g ~dim:k ~pos:i)
          || IntSet.is_empty (cofaces_of `Output g ~dim:k ~pos:i)
        then acc := IntSet.add i !acc
      done
      ; !acc

let maximal (k : int) (g : t) : intset =
  let n = Array.length g.faces_in.(k) in
  let acc = ref IntSet.empty in
  for i = 0 to n - 1 do
    if IntSet.is_empty (cofaces_of `Both g ~dim:k ~pos:i) then
      acc := IntSet.add i !acc
  done
  ; !acc

let is_pure (g : t) : bool =
  let n = g.dim in
  if n <= 1 then true
  else
    let rec aux k =
      if k >= n then true else IntSet.is_empty (maximal k g) && aux (k + 1)
    in
    aux 0

let is_atom (g : t) : bool =
  is_pure g
  &&
  let sz = sizes g in
  let top = g.dim in
  if top < 0 then false else sz.(top) = 1

let stack_extremal sign shape =
  let rec aux acc k =
    if k < 0 then acc else aux ((k, extremal sign k shape) :: acc) (k - 1)
  in
  aux [] (dim shape)

let build_stack_paste sign shape max_dim =
  let rec aux acc k =
    if k < 0 then acc else aux ((k, extremal sign k shape) :: acc) (k - 1)
  in
  aux [] max_dim |> List.rev

let build_stack_cellN shape =
  let d = dim shape in
  let rec gather acc k =
    if k < 0 then acc
    else
      let set = extremal `Input k shape in
      gather ((k, set) :: acc) (k - 1)
  in
  let inputs = gather [] (d - 1) in
  if d > 0 then inputs @ [ (d - 1, extremal `Output (d - 1) shape) ] else inputs

let is_round (g : t) : bool =
  let n = g.dim in
  if n <= 1 then true
  else if not (is_pure g) then false
  else
    let sz = sizes g in
    if sz.(n) = 1 then true
    else
      (* Propagate input/output interior cells dimension by dimension, removing
         elements already claimed by earlier interiors. *)
      let in_interior = Array.make n [||] in
      let out_interior = Array.make n [||] in
      let accum_in = Array.make n IntSet.empty in
      let accum_out = Array.make n IntSet.empty in
      let rec step j =
        if j >= n then true
        else
          let build_layer base =
            let layer = Array.init (j + 1) (fun _ -> IntSet.empty) in
            layer.(j) <- base
            ; for i = j - 1 downto 0 do
                IntSet.iter
                  (fun p ->
                    let faces = faces_of `Both g ~dim:(i + 1) ~pos:p in
                    layer.(i) <- IntSet.union layer.(i) faces)
                  layer.(i + 1)
                ; let prev = IntSet.union accum_in.(i) accum_out.(i) in
                  layer.(i) <- IntSet.diff layer.(i) prev
              done
            ; layer
          in
          let layer_in = build_layer (extremal `Input j g)
          and layer_out = build_layer (extremal `Output j g) in
          let rec has_overlap i =
            if i < 0 then false
            else
              let overlap = IntSet.inter layer_in.(i) layer_out.(i) in
              if IntSet.is_empty overlap then has_overlap (i - 1) else true
          in
          if has_overlap j then false
          else (
            in_interior.(j) <- layer_in
            ; out_interior.(j) <- layer_out
            ; for i = 0 to j do
                accum_in.(i) <- IntSet.union accum_in.(i) layer_in.(i)
              done
            ; for i = 0 to j do
                accum_out.(i) <- IntSet.union accum_out.(i) layer_out.(i)
              done
            ; step (j + 1))
      in
      step 0

type embedding_data = {
  forward: int array array; (* boundary index -> original index in g *)
  inv_dom: int array array; (* original index in g -> boundary index, or -1 *)
}

let boundary (s : sign) (k : int) (g : t) : t * Embedding.t =
  if k >= g.dim then (g, Embedding.id g)
  else if k < 0 then (empty, Embedding.empty g)
  else
    let dims_b = k + 1 in
    let sz_g = sizes g in

    (* Accumulators: per-dimension lists in insertion order (we’ll reverse
       once). *)
    let acc : int list array = Array.init dims_b (fun _ -> []) in
    let inv_dom : int array array =
      Array.init dims_b (fun j -> Array.make sz_g.(j) (-1))
    in
    let next_new_index : int array = Array.make dims_b 0 in

    (* Insert into F at level j; also update inv and next_new_index. *)
    let insert_F (j : int) (old : int) =
      let i = next_new_index.(j) in
      inv_dom.(j).(old) <- i
      ; acc.(j) <- old :: acc.(j)
      ; next_new_index.(j) <- i + 1
    in

    (* Step 1: level k — add extremal s k g, no membership check needed *)
    IntSet.iter (fun i -> insert_F k i) (extremal s k g)

    ; (* Step 2: recursively fill lower levels *)
      for j = k - 1 downto 0 do
        (* (1) faces of (j+1)-cells already in F: need membership checks via
           inv *)
        let parents = List.rev acc.(j + 1) in
        (* reverse to preserve insertion order *)
        List.iter
          (fun parent_old ->
            IntSet.iter
              (fun f -> if inv_dom.(j).(f) < 0 then insert_F j f)
              (faces_of `Both g ~dim:(j + 1) ~pos:parent_old))
          parents
        ; (* (2) maximal j-cells: no checks needed (cannot be faces of
             (j+1)-cells) *)
          IntSet.iter (fun m -> insert_F j m) (maximal j g)
      done

    ; (* Finalize forward map by reversing accumulators to restore insertion
         order. *)
      let forward : int array array =
        Array.init dims_b (fun j -> acc.(j) |> List.rev |> Array.of_list)
      in

      let ed : embedding_data = { forward; inv_dom } in

      (* Build boundary adjacencies using the shared helper. *)
      let faces_in' =
        remap_adjacency ~levels:dims_b ~forward:ed.forward ~inv_dom:ed.inv_dom
          ~shift:(-1) g.faces_in
      and faces_out' =
        remap_adjacency ~levels:dims_b ~forward:ed.forward ~inv_dom:ed.inv_dom
          ~shift:(-1) g.faces_out
      and cofaces_in' =
        remap_adjacency ~levels:dims_b ~forward:ed.forward ~inv_dom:ed.inv_dom
          ~shift:1 g.cofaces_in
      and cofaces_out' =
        remap_adjacency ~levels:dims_b ~forward:ed.forward ~inv_dom:ed.inv_dom
          ~shift:1 g.cofaces_out
      in

      (* Assemble boundary ogposet and inclusion embedding *)
      let sub =
        {
          dim= k;
          faces_in= faces_in';
          faces_out= faces_out';
          cofaces_in= cofaces_in';
          cofaces_out= cofaces_out';
          normal= false;
        }
      in
      let cod_inv =
        Array.init (Array.length sz_g) (fun d ->
            if d < dims_b then ed.inv_dom.(d) else Array.make sz_g.(d) (-1))
      in
      let emb = Embedding.make ~dom:sub ~cod:g ~map:ed.forward ~inv:cod_inv in
      (sub, emb)

let traverse (g : t) (initial_stack : (int * intset) list) : t * Embedding.t =
  match initial_stack with
  | [] ->
      (empty, Embedding.empty g)
  | _ ->
      let dims = g.dim + 1 in
      let sizes_g = sizes g in
      (* Downward closure of the requested cells, one intset per dimension. *)
      let max_dim =
        List.fold_left
          (fun acc (d, _) -> if d > acc then d else acc)
          (-1) initial_stack
      in
      let dc = Array.make (max_dim + 1) IntSet.empty in
      List.iter
        (fun (d, cells) -> dc.(d) <- IntSet.union dc.(d) cells)
        initial_stack
      ; for d = max_dim downto 1 do
          if d <= g.dim then (
            let lower = ref dc.(d - 1) in
            IntSet.iter
              (fun cell ->
                lower := IntSet.union !lower g.faces_in.(d).(cell)
                ; lower := IntSet.union !lower g.faces_out.(d).(cell))
              dc.(d)
            ; dc.(d - 1) <- !lower)
        done
      ; let map_levels = max_dim + 1 in
        let map_sizes =
          Array.init map_levels (fun d -> IntSet.cardinal dc.(d))
        in
        let map = Array.init map_levels (fun d -> Array.make map_sizes.(d) (-1))
        and next_idx = Array.make map_levels 0
        and inv = Array.init dims (fun d -> Array.make sizes_g.(d) (-1)) in
        let mark dim cell =
          let idx = next_idx.(dim) in
          map.(dim).(idx) <- cell
          ; inv.(dim).(cell) <- idx
          ; next_idx.(dim) <- idx + 1
        in
        let union_faces faces dim elements =
          IntSet.fold
            (fun cell acc -> IntSet.union acc faces.(dim).(cell))
            elements IntSet.empty
        in
        let rec loop stack =
          match stack with
          | [] ->
              ()
          | (dim, focus) :: rest -> (
              if IntSet.is_empty focus then loop rest
              else if IntSet.for_all (fun p -> inv.(dim).(p) >= 0) focus then
                loop rest
              else if dim = 0 then (
                IntSet.iter (fun p -> if inv.(0).(p) < 0 then mark 0 p) focus
                ; loop rest)
              else
                let focus_in = union_faces g.faces_in dim focus
                and focus_out = union_faces g.faces_out dim focus in
                let focus_input = IntSet.diff focus_in focus_out in
                if IntSet.exists (fun p -> inv.(dim - 1).(p) < 0) focus_input
                then loop ((dim - 1, focus_input) :: stack)
                else if IntSet.cardinal focus = 1 then (
                  let q = IntSet.min_elt focus in
                  mark dim q
                  ; let outputs = g.faces_out.(dim).(q) in
                    if IntSet.exists (fun p -> inv.(dim - 1).(p) < 0) outputs
                    then loop ((dim - 1, outputs) :: rest)
                    else loop rest)
                else
                  let candidate =
                    IntSet.fold
                      (fun x acc ->
                        let order = inv.(dim - 1).(x) in
                        if order < 0 then acc
                        else
                          let cofaces = g.cofaces_in.(dim - 1).(x) in
                          let unmarked =
                            cofaces |> IntSet.inter focus
                            |> IntSet.filter (fun q -> inv.(dim).(q) < 0)
                          in
                          if IntSet.is_empty unmarked then acc
                          else
                            let q = IntSet.min_elt unmarked in
                            match acc with
                            | None ->
                                Some (order, q)
                            | Some (best_order, best_q) ->
                                if order < best_order then Some (order, q)
                                else if order = best_order && q < best_q then
                                  Some (order, q)
                                else acc)
                      focus_in None
                  in
                  match candidate with
                  | Some (_, q) ->
                      loop ((dim, IntSet.singleton q) :: stack)
                  | None -> (
                      (* Should not happen with well-formed inputs, but mark one
                         element to ensure progress. *)
                      let fallback =
                        let exception Found of int in
                        try
                          IntSet.iter
                            (fun q -> if inv.(dim).(q) < 0 then raise (Found q))
                            focus
                          ; None
                        with Found q -> Some q
                      in
                      match fallback with
                      | Some q ->
                          mark dim q ; loop stack
                      | None ->
                          loop rest))
        in
        loop initial_stack
        ; let ed =
            { forward= map; inv_dom= Array.init map_levels (fun d -> inv.(d)) }
          in
          let faces_in' =
            remap_adjacency ~levels:map_levels ~forward:ed.forward
              ~inv_dom:ed.inv_dom ~shift:(-1) g.faces_in
          and faces_out' =
            remap_adjacency ~levels:map_levels ~forward:ed.forward
              ~inv_dom:ed.inv_dom ~shift:(-1) g.faces_out
          and cofaces_in' =
            remap_adjacency ~levels:map_levels ~forward:ed.forward
              ~inv_dom:ed.inv_dom ~shift:1 g.cofaces_in
          and cofaces_out' =
            remap_adjacency ~levels:map_levels ~forward:ed.forward
              ~inv_dom:ed.inv_dom ~shift:1 g.cofaces_out
          in
          let dom =
            {
              dim= max_dim;
              faces_in= faces_in';
              faces_out= faces_out';
              cofaces_in= cofaces_in';
              cofaces_out= cofaces_out';
              normal= false;
            }
          in
          let emb = Embedding.make ~dom ~cod:g ~map ~inv in
          (dom, emb)

let normalisation g =
  if is_normal g then (g, Embedding.id g)
  else
    let stack = stack_extremal `Input g in
    let dom, emb = traverse g stack in
    ({ dom with normal= true }, emb)

let boundary_traverse sign k g =
  match sign with
  | `Input ->
      let stack = build_stack_paste `Input g (min k (dim g)) in
      let dom, emb = traverse g stack in
      ({ dom with normal= true }, emb)
  | `Output ->
      let stack = build_stack_paste `Output g (min k (dim g)) in
      let dom, emb = traverse g stack in
      ({ dom with normal= true }, emb)
  | `Both ->
      let stack = build_stack_cellN g in
      traverse g stack

let isomorphism_of (u : t) (v : t) : Embedding.t checked =
  let failure msg = Error (Error.make msg) in
  if u == v then Ok (Embedding.id u)
  else
    let dim_u = dim u and dim_v = dim v in
    if dim_u <> dim_v then failure "dimensions do not match"
    else
      let sizes_u = sizes u and sizes_v = sizes v in
      if sizes_u <> sizes_v then failure "shapes do not match"
      else if equal u v then Ok (Embedding.id u)
      else
        let u', e_u = normalisation u and v', e_v = normalisation v in
        if not (equal u' v') then failure "canonical forms do not match"
        else
          let compose () =
            let inv_u = Embedding.inv e_u
            and map_u = Embedding.map e_u
            and map_v = Embedding.map e_v
            and inv_v = Embedding.inv e_v in
            let dims_dom = Array.length inv_u in
            let dims_cod = Array.length inv_v in
            if dims_dom <> Array.length map_v || dims_cod <> Array.length map_u
            then failure "failed to compose isomorphism data"
            else
              let produce_rows inv_levels map_levels =
                let dims = Array.length inv_levels in
                let result = Array.make dims [||] in
                let ok = ref true in
                for dim = 0 to dims - 1 do
                  if !ok then (
                    let inv_level = inv_levels.(dim) in
                    let map_level = map_levels.(dim) in
                    let len = Array.length inv_level in
                    let row = Array.make len (-1) in
                    for idx = 0 to len - 1 do
                      let mid = inv_level.(idx) in
                      if mid < 0 || mid >= Array.length map_level then
                        ok := false
                      else row.(idx) <- map_level.(mid)
                    done
                    ; result.(dim) <- row)
                done
                ; if !ok then Ok result
                  else failure "failed to compose isomorphism data"
              in
              match produce_rows inv_u map_v with
              | Error _ as e ->
                  e
              | Ok map -> (
                  match produce_rows inv_v map_u with
                  | Error _ as e ->
                      e
                  | Ok inv ->
                      Ok (Embedding.make ~dom:u ~cod:v ~map ~inv))
          in
          compose ()

let isomorphic (u : t) (v : t) : bool =
  if u == v then true
  else
    let dim_u = dim u and dim_v = dim v in
    if dim_u <> dim_v then false
    else
      let sizes_u = sizes u and sizes_v = sizes v in
      if sizes_u <> sizes_v then false
      else if equal u v then true
      else
        let u', _ = normalisation u and v', _ = normalisation v in
        equal u' v'

type pushout = { tip: t; inl: Embedding.t; inr: Embedding.t }

let attach (f : Embedding.t) (g : Embedding.t) : pushout =
  let b = Embedding.cod f
  and c = Embedding.cod g
  and f_map = Embedding.map f
  and g_inv = Embedding.inv g in
  let tip_dim = max b.dim c.dim in
  let levels = tip_dim + 1 in
  (* Pre-compute how many slots the pushout will require in each dimension. *)
  let base_sizes =
    Array.init levels (fun d ->
        if d <= b.dim then Array.length b.faces_in.(d) else 0)
  in
  let extra_counts = Array.make levels 0 in
  for i = 0 to c.dim do
    let faces_in_c = c.faces_in.(i) in
    let len = Array.length faces_in_c in
    let g_inv_i = if i < Array.length g_inv then g_inv.(i) else [||] in
    for p = 0 to len - 1 do
      let preimage = if p < Array.length g_inv_i then g_inv_i.(p) else -1 in
      if preimage < 0 then extra_counts.(i) <- extra_counts.(i) + 1
    done
  done
  ; let total_sizes =
      Array.init levels (fun d -> base_sizes.(d) + extra_counts.(d))
    in
    (* Allocate the adjacency families to their final size, copying b’s
       prefix. *)
    let alloc_faces base =
      Array.init levels (fun d ->
          let total = total_sizes.(d) in
          if total = 0 then [||]
          else
            let arr = Array.make total IntSet.empty in
            (if d <= b.dim then
               let src = base.(d) in
               Array.blit src 0 arr 0 (Array.length src))
            ; arr)
    in
    let tip_faces_in = alloc_faces b.faces_in in
    let tip_faces_out = alloc_faces b.faces_out in
    let tip_cofaces_in = alloc_faces b.cofaces_in in
    let tip_cofaces_out = alloc_faces b.cofaces_out in
    let inr_inv =
      Array.init levels (fun d -> Array.make total_sizes.(d) (-1))
    in
    let counters = Array.copy base_sizes in
    let inr_map =
      let len = c.dim + 1 in
      Array.init len (fun d ->
          if d <= c.dim then Array.make (Array.length c.faces_in.(d)) (-1)
          else [||])
    in
    for i = 0 to c.dim do
      let map_i = inr_map.(i) in
      let faces_in_c = c.faces_in.(i) and faces_out_c = c.faces_out.(i) in
      let len = Array.length faces_in_c in
      let g_inv_i = if i < Array.length g_inv then g_inv.(i) else [||] in
      for p = 0 to len - 1 do
        let preimage = if p < Array.length g_inv_i then g_inv_i.(p) else -1 in
        if preimage >= 0 then (
          let target = f_map.(i).(preimage) in
          map_i.(p) <- target
          ; if i < Array.length inr_inv then inr_inv.(i).(target) <- p)
        else
          let idx = counters.(i) in
          map_i.(p) <- idx
          ; let new_faces_in =
              if i = 0 then IntSet.empty
              else set_map (fun q -> inr_map.(i - 1).(q)) faces_in_c.(p)
            in
            let new_faces_out =
              if i = 0 then IntSet.empty
              else set_map (fun q -> inr_map.(i - 1).(q)) faces_out_c.(p)
            in
            (* Drop the new cell into place and update coface links. *)
            tip_faces_in.(i).(idx) <- new_faces_in
            ; tip_faces_out.(i).(idx) <- new_faces_out
            ; inr_inv.(i).(idx) <- p
            ; if i > 0 then (
                IntSet.iter
                  (fun q ->
                    tip_cofaces_in.(i - 1).(q) <-
                      IntSet.add idx tip_cofaces_in.(i - 1).(q))
                  new_faces_in
                ; IntSet.iter
                    (fun q ->
                      tip_cofaces_out.(i - 1).(q) <-
                        IntSet.add idx tip_cofaces_out.(i - 1).(q))
                    new_faces_out)
            ; counters.(i) <- idx + 1
      done
    done
    ; let tip =
        {
          dim= tip_dim;
          faces_in= tip_faces_in;
          faces_out= tip_faces_out;
          cofaces_in= tip_cofaces_in;
          cofaces_out= tip_cofaces_out;
          normal= false;
        }
      in
      let tip_sizes = sizes tip in
      let inl_map =
        Array.init (b.dim + 1) (fun d ->
            Array.init (Array.length b.faces_in.(d)) (fun i -> i))
      in
      (* Extend `inl`’s inverse arrays so they cover the whole tip. *)
      let inl_inv =
        Array.init levels (fun d ->
            let size_tip = tip_sizes.(d) in
            let arr = Array.make size_tip (-1) in
            (if d <= b.dim then
               let size_b = Array.length b.faces_in.(d) in
               for i = 0 to min size_b size_tip - 1 do
                 arr.(i) <- i
               done)
            ; arr)
      in
      let inl = Embedding.make ~dom:b ~cod:tip ~map:inl_map ~inv:inl_inv in
      let inr = Embedding.make ~dom:c ~cod:tip ~map:inr_map ~inv:inr_inv in
      { tip; inl; inr }

let pushout (f : Embedding.t) (g : Embedding.t) : pushout =
  let size_sum x = Array.fold_left ( + ) 0 (sizes x) in
  let b = Embedding.cod f and c = Embedding.cod g in
  if size_sum b >= size_sum c then attach f g
  else
    let res = attach g f in
    { tip= res.tip; inl= res.inr; inr= res.inl }
