(* ogposet.ml — implementation for oriented graded posets with pluggable IntSet
   backend *)

(* You may redefine IntSet below to use another integer-set backend, e.g. a
   bitset or hybrid. *)
module IntSet = Set.Make (Int)

(* --- Types --- *)

type elt = { dim: int; pos: int }
type faces = IntSet.t array array

type t = {
  max_dim: int;
  size: int array;
  inputs: faces;
  outputs: faces;
  coinputs: faces;
  cooutputs: faces;
}

type poset = t

module Sub = struct
  module IntMap = Map.Make (Int)

  type t = { dims: IntSet.t; cells: IntSet.t IntMap.t }

  let empty = { dims= IntSet.empty; cells= IntMap.empty }

  let lookup dim cells =
    try IntMap.find dim cells with Not_found -> IntSet.empty

  let positions (s : t) ~dim = lookup dim s.cells

  let add_position (s : t) ~dim ~pos =
    let updated = IntSet.add pos (positions s ~dim) in
    { dims= IntSet.add dim s.dims; cells= IntMap.add dim updated s.cells }

  let mem_position (s : t) ~dim ~pos = IntSet.mem pos (positions s ~dim)

  let iter (s : t) ~f =
    IntSet.iter
      (fun dim -> IntSet.iter (fun pos -> f ~dim ~pos) (positions s ~dim))
      s.dims

  let of_list (els : elt list) : t =
    List.fold_left
      (fun acc e -> add_position acc ~dim:e.dim ~pos:e.pos)
      empty els

  let of_dim_set ~dim (set : IntSet.t) : t =
    if IntSet.is_empty set then empty
    else { dims= IntSet.singleton dim; cells= IntMap.add dim set IntMap.empty }

  let of_dim ~dim (s : t) : t =
    let set = positions s ~dim in
    if IntSet.is_empty set then empty
    else { dims= IntSet.singleton dim; cells= IntMap.add dim set IntMap.empty }

  let union (a : t) (b : t) : t =
    let dims = IntSet.union a.dims b.dims in
    let cells =
      IntSet.fold
        (fun dim acc ->
          let merged = IntSet.union (positions a ~dim) (positions b ~dim) in
          if IntSet.is_empty merged then acc else IntMap.add dim merged acc)
        dims IntMap.empty
    in
    { dims; cells }

  let intersection (a : t) (b : t) : t =
    IntSet.fold
      (fun dim acc ->
        let inter = IntSet.inter (positions a ~dim) (positions b ~dim) in
        if IntSet.is_empty inter then acc
        else
          {
            dims= IntSet.add dim acc.dims;
            cells= IntMap.add dim inter acc.cells;
          })
      (IntSet.inter a.dims b.dims)
      empty
end

module Embedding = struct
  type t = { dom: poset; cod: poset; map: int array array }

  let dom e = e.dom
  let cod e = e.cod

  let compose (f : t) (g : t) : t =
    let dims = Array.length f.map in
    let map =
      Array.init dims (fun n ->
          let dom_len = Array.length f.map.(n) in
          let arr = Array.make dom_len 0 in
          for i = 0 to dom_len - 1 do
            let mid = f.map.(n).(i) in
            arr.(i) <- g.map.(n).(mid)
          done
          ; arr)
    in
    { dom= f.dom; cod= g.cod; map }
end

(* --- Helpers --- *)
let dim_count g = g.max_dim + 1
let size_in g n = if n < 0 || n > g.max_dim then 0 else g.size.(n)

(* --- Construction with incremental coface maintenance --- *)

let empty : t =
  {
    max_dim= -1;
    size= [||];
    inputs= [||];
    outputs= [||];
    coinputs= [||];
    cooutputs= [||];
  }

let extend_to_dim (g : t) (n : int) : t =
  if n <= g.max_dim then g
  else
    let new_dims = n + 1 in
    let grow_faces (a : faces) =
      let arr = Array.make new_dims [||] in
      Array.blit a 0 arr 0 (Array.length a)
      ; arr
    in
    let size' = Array.make new_dims 0 in
    Array.blit g.size 0 size' 0 (Array.length g.size)
    ; {
        max_dim= n;
        size= size';
        inputs= grow_faces g.inputs;
        outputs= grow_faces g.outputs;
        coinputs= grow_faces g.coinputs;
        cooutputs= grow_faces g.cooutputs;
      }

let ensure_layer_capacity arr new_len =
  if Array.length arr < new_len then (
    let a = Array.make new_len IntSet.empty in
    Array.blit arr 0 a 0 (Array.length arr)
    ; a)
  else arr

let add0 (g : t) (m : int) : t * elt list =
  let g = extend_to_dim g 0 in
  let old = if g.max_dim < 0 then 0 else g.size.(0) in
  let size' = Array.copy g.size in
  size'.(0) <- old + m
  ; let coinputs' = Array.copy g.coinputs in
    let cooutputs' = Array.copy g.cooutputs in
    if Array.length coinputs'.(0) < size'.(0) then
      coinputs'.(0) <- ensure_layer_capacity coinputs'.(0) size'.(0)
    ; if Array.length cooutputs'.(0) < size'.(0) then
        cooutputs'.(0) <- ensure_layer_capacity cooutputs'.(0) size'.(0)
    ; let g' =
        {
          g with
          max_dim= max g.max_dim 0;
          size= size';
          coinputs= coinputs';
          cooutputs= cooutputs';
        }
      in
      let elts = List.init m (fun i -> { dim= 0; pos= old + i }) in
      (g', elts)

let addN (g : t) ~(dim : int) ~(inputs : Sub.t list) ~(outputs : Sub.t list) :
    t * elt list =
  let g = extend_to_dim g dim in
  let n_old = size_in g dim in
  let k = List.length inputs in
  let n_new = n_old + k in
  let size' = Array.copy g.size in
  size'.(dim) <- n_new
  ; let inputs' = Array.copy g.inputs in
    let outputs' = Array.copy g.outputs in
    if dim > 0 then (
      inputs'.(dim) <- ensure_layer_capacity inputs'.(dim) n_new
      ; outputs'.(dim) <- ensure_layer_capacity outputs'.(dim) n_new)
    ; let coinputs' = Array.copy g.coinputs in
      let cooutputs' = Array.copy g.cooutputs in
      if dim - 1 >= 0 then (
        coinputs'.(dim - 1) <-
          ensure_layer_capacity coinputs'.(dim - 1) (size_in g (dim - 1))
        ; cooutputs'.(dim - 1) <-
            ensure_layer_capacity cooutputs'.(dim - 1) (size_in g (dim - 1)))
      ; let allowed_dim = dim - 1 in
        let faces_in_dim (sub : Sub.t) = Sub.positions sub ~dim:allowed_dim in
        List.iteri
          (fun i sub ->
            let set = faces_in_dim sub in
            inputs'.(dim).(n_old + i) <- set
            ; IntSet.iter
                (fun j ->
                  coinputs'.(allowed_dim).(j) <-
                    IntSet.add (n_old + i) coinputs'.(allowed_dim).(j))
                set)
          inputs
        ; List.iteri
            (fun i sub ->
              let set = faces_in_dim sub in
              outputs'.(dim).(n_old + i) <- set
              ; IntSet.iter
                  (fun j ->
                    cooutputs'.(allowed_dim).(j) <-
                      IntSet.add (n_old + i) cooutputs'.(allowed_dim).(j))
                  set)
            outputs
        ; let g' =
            {
              g with
              size= size';
              inputs= inputs';
              outputs= outputs';
              coinputs= coinputs';
              cooutputs= cooutputs';
            }
          in
          let elts = List.init k (fun i -> { dim; pos= n_old + i }) in
          (g', elts)

(* --- Accessors --- *)

let get_faces (_g : t) arr (e : elt) : Sub.t =
  if e.dim = 0 then Sub.empty
  else Sub.of_dim_set ~dim:(e.dim - 1) arr.(e.dim).(e.pos)

let get_cofaces (g : t) arr (e : elt) : Sub.t =
  let next_dim = e.dim + 1 in
  if next_dim > g.max_dim then Sub.empty
  else Sub.of_dim_set ~dim:next_dim arr.(e.dim).(e.pos)

let inputs g e = get_faces g g.inputs e
let outputs g e = get_faces g g.outputs e

let coinputs g e =
  if e.dim >= g.max_dim then Sub.empty else get_cofaces g g.coinputs e

let cooutputs g e =
  if e.dim >= g.max_dim then Sub.empty else get_cofaces g g.cooutputs e

(* --- Closure (downward) --- *)

let closure (g : t) (s : Sub.t) : Sub.t =
  if g.max_dim < 0 then s
  else
    let q = Array.init (dim_count g) (fun _ -> Queue.create ()) in
    Sub.iter s ~f:(fun ~dim ~pos -> Queue.add pos q.(dim))
    ; let res = ref s in
      for n = g.max_dim downto 0 do
        while not (Queue.is_empty q.(n)) do
          let i = Queue.take q.(n) in
          if n > 0 then (
            let add_faces faces =
              IntSet.iter
                (fun j ->
                  if not (Sub.mem_position !res ~dim:(n - 1) ~pos:j) then (
                    res := Sub.add_position !res ~dim:(n - 1) ~pos:j
                    ; Queue.add j q.(n - 1)))
                faces.(n).(i)
            in
            add_faces g.inputs ; add_faces g.outputs)
        done
      done
      ; !res

(* --- Embed --- *)

let embed (g : t) (s : Sub.t) : t * Embedding.t =
  let dims = dim_count g in
  let map = Array.init dims (fun n -> Array.make g.size.(n) (-1)) in
  let size' = Array.make dims 0 in
  for n = 0 to g.max_dim do
    let elements = IntSet.elements (Sub.positions s ~dim:n) in
    size'.(n) <- List.length elements
    ; List.iteri (fun idx i_old -> map.(n).(i_old) <- idx) elements
  done
  ; let inputs' =
      Array.init dims (fun n ->
          if n = 0 then [||] else Array.make size'.(n) IntSet.empty)
    in
    let outputs' =
      Array.init dims (fun n ->
          if n = 0 then [||] else Array.make size'.(n) IntSet.empty)
    in
    let remap_faces n set =
      IntSet.fold (fun j acc -> IntSet.add map.(n - 1).(j) acc) set IntSet.empty
    in
    for n = 1 to g.max_dim do
      let elements = IntSet.elements (Sub.positions s ~dim:n) in
      List.iteri
        (fun idx i_old ->
          inputs'.(n).(idx) <- remap_faces n g.inputs.(n).(i_old)
          ; outputs'.(n).(idx) <- remap_faces n g.outputs.(n).(i_old))
        elements
    done
    ; let coinputs' =
        Array.init dims (fun n -> Array.make size'.(n) IntSet.empty)
      in
      let cooutputs' =
        Array.init dims (fun n -> Array.make size'.(n) IntSet.empty)
      in
      for n = 1 to g.max_dim do
        for i = 0 to size'.(n) - 1 do
          IntSet.iter
            (fun j ->
              coinputs'.(n - 1).(j) <- IntSet.add i coinputs'.(n - 1).(j))
            inputs'.(n).(i)
          ; IntSet.iter
              (fun j ->
                cooutputs'.(n - 1).(j) <- IntSet.add i cooutputs'.(n - 1).(j))
              outputs'.(n).(i)
        done
      done
      ; let h =
          {
            max_dim= g.max_dim;
            size= size';
            inputs= inputs';
            outputs= outputs';
            coinputs= coinputs';
            cooutputs= cooutputs';
          }
        in
        let emb = Embedding.{ dom= h; cod= g; map } in
        (h, emb)

(* --- Boundaries --- *)

let has_any_coface g n i =
  if n < g.max_dim then
    not
      (IntSet.is_empty g.coinputs.(n).(i) && IntSet.is_empty g.cooutputs.(n).(i))
  else false

let is_input_face g k i =
  if k < g.max_dim then IntSet.is_empty g.cooutputs.(k).(i) else true

let is_output_face g k i =
  if k < g.max_dim then IntSet.is_empty g.coinputs.(k).(i) else true

let boundary (g : t) (k : int) face_pred =
  if k > g.max_dim then Sub.empty
  else
    let subset = ref Sub.empty in
    for i = 0 to g.size.(k) - 1 do
      if face_pred k i then subset := Sub.add_position !subset ~dim:k ~pos:i
    done
    ; for j = 0 to k - 1 do
        for i = 0 to g.size.(j) - 1 do
          if not (has_any_coface g j i) then
            subset := Sub.add_position !subset ~dim:j ~pos:i
        done
      done
    ; closure g !subset

let bd_in (g : t) (k : int) : Sub.t = boundary g k (is_input_face g)
let bd_out (g : t) (k : int) : Sub.t = boundary g k (is_output_face g)

(* --- Fast asymmetric pushout: attach complement of g(C) in B onto A --- *)

let attach (f : Embedding.t) (g : Embedding.t) : t * Embedding.t * Embedding.t =
  let a = f.cod and b = g.cod and c = f.dom in
  let size_a n = if n <= a.max_dim then a.size.(n) else 0 in
  let size_b n = if n <= b.max_dim then b.size.(n) else 0 in
  let dims = max (a.max_dim + 1) (b.max_dim + 1) in
  let inv_g = Array.init dims (fun n -> Array.make (size_b n) (-1)) in
  for n = 0 to c.max_dim do
    for i = 0 to c.size.(n) - 1 do
      let jB = g.map.(n).(i) in
      inv_g.(n).(jB) <- i
    done
  done
  ; let p = ref a in
    let map_b_to_p = Array.init dims (fun n -> Array.make (size_b n) (-1)) in
    for n = 0 to b.max_dim do
      let comp = ref [] in
      for j = 0 to size_b n - 1 do
        if inv_g.(n).(j) < 0 then comp := j :: !comp
      done
      ; if !comp <> [] then (
          if n = 0 then (
            let count = List.length !comp in
            let p_after, new_elts = add0 !p count in
            p := p_after
            ; List.iter2
                (fun j e -> map_b_to_p.(0).(j) <- e.pos)
                (List.rev !comp) new_elts)
          else
            let in_subs = ref [] and out_subs = ref [] in
            let order = List.rev !comp in
            List.iter
              (fun j ->
                let map_face set =
                  IntSet.fold
                    (fun k acc ->
                      if k < size_b (n - 1) && inv_g.(n - 1).(k) >= 0 then
                        let c = inv_g.(n - 1).(k) in
                        let a_pos = f.map.(n - 1).(c) in
                        Sub.add_position acc ~dim:(n - 1) ~pos:a_pos
                      else
                        let ppos = map_b_to_p.(n - 1).(k) in
                        Sub.add_position acc ~dim:(n - 1) ~pos:ppos)
                    set Sub.empty
                in
                in_subs := map_face b.inputs.(n).(j) :: !in_subs
                ; out_subs := map_face b.outputs.(n).(j) :: !out_subs)
              order
            ; let p_after, new_elts =
                addN !p ~dim:n ~inputs:(List.rev !in_subs)
                  ~outputs:(List.rev !out_subs)
              in
              p := p_after
              ; List.iter2
                  (fun j e -> map_b_to_p.(n).(j) <- e.pos)
                  order new_elts)
    done
    ; let p_final = !p in
      let map_a =
        Array.init dims (fun n -> Array.init (size_a n) (fun i -> i))
      in
      let map_b =
        Array.init dims (fun n ->
            Array.init (size_b n) (fun j ->
                if j < size_b n && inv_g.(n).(j) >= 0 then
                  let c = inv_g.(n).(j) in
                  f.map.(n).(c)
                else map_b_to_p.(n).(j)))
      in
      let iota_a = Embedding.{ dom= a; cod= p_final; map= map_a } in
      let iota_b = Embedding.{ dom= b; cod= p_final; map= map_b } in
      (p_final, iota_a, iota_b)

(* --- Heuristic pushout: choose cheaper direction --- *)

let pushout (f : Embedding.t) (g : Embedding.t) : t * Embedding.t * Embedding.t
    =
  let a = f.cod and b = g.cod and c = f.dom in
  let size_total x =
    let s = ref 0 in
    for n = 0 to x.max_dim do
      s := !s + x.size.(n)
    done
    ; !s
  in
  let a_tot = size_total a and b_tot = size_total b and c_tot = size_total c in
  let add_B_into_A = b_tot - c_tot in
  let add_A_into_B = a_tot - c_tot in
  if add_B_into_A <= add_A_into_B then attach f g
  else
    let p, i_b, i_a = attach g f in
    (p, i_a, i_b)

let coequaliser (f : Embedding.t) (g : Embedding.t) :
    t * Embedding.t * Embedding.t =
  let a = f.dom and b = f.cod in
  let dims = b.max_dim + 1 in
  let parents = Array.init dims (fun n -> Array.init b.size.(n) (fun i -> i)) in
  let rec find parent i =
    let p = parent.(i) in
    if p = i then i
    else
      let root = find parent p in
      parent.(i) <- root
      ; root
  in
  let union parent x y =
    let rx = find parent x in
    let ry = find parent y in
    if rx <> ry then parent.(rx) <- ry
  in
  for n = 0 to a.max_dim do
    let parent = parents.(n) in
    for i = 0 to a.size.(n) - 1 do
      union parent f.map.(n).(i) g.map.(n).(i)
    done
  done
  ; let class_map = Array.make dims [||] in
    let new_size = Array.make dims 0 in
    for n = 0 to dims - 1 do
      let parent = parents.(n) in
      let size_n = Array.length parent in
      let mapping = Array.make size_n 0 in
      let repr_index = Array.make size_n (-1) in
      let next = ref 0 in
      for i = 0 to size_n - 1 do
        let root = find parent i in
        let idx =
          let existing = repr_index.(root) in
          if existing >= 0 then existing
          else
            let v = !next in
            incr next
            ; repr_index.(root) <- v
            ; v
        in
        mapping.(i) <- idx
      done
      ; class_map.(n) <- mapping
      ; new_size.(n) <- !next
    done
    ; let inputs =
        Array.init dims (fun n ->
            if n = 0 then [||] else Array.make new_size.(n) IntSet.empty)
      in
      let outputs =
        Array.init dims (fun n ->
            if n = 0 then [||] else Array.make new_size.(n) IntSet.empty)
      in
      for n = 1 to b.max_dim do
        for i = 0 to b.size.(n) - 1 do
          let target = class_map.(n).(i) in
          let mapped_inputs =
            IntSet.fold
              (fun j acc -> IntSet.add class_map.(n - 1).(j) acc)
              b.inputs.(n).(i)
              IntSet.empty
          in
          let mapped_outputs =
            IntSet.fold
              (fun j acc -> IntSet.add class_map.(n - 1).(j) acc)
              b.outputs.(n).(i)
              IntSet.empty
          in
          inputs.(n).(target) <- IntSet.union inputs.(n).(target) mapped_inputs
          ; outputs.(n).(target) <-
              IntSet.union outputs.(n).(target) mapped_outputs
        done
      done
      ; let coinputs =
          Array.init dims (fun n -> Array.make new_size.(n) IntSet.empty)
        and cooutputs =
          Array.init dims (fun n -> Array.make new_size.(n) IntSet.empty)
        in
        for n = 1 to b.max_dim do
          for i = 0 to new_size.(n) - 1 do
            IntSet.iter
              (fun j ->
                coinputs.(n - 1).(j) <- IntSet.add i coinputs.(n - 1).(j))
              inputs.(n).(i)
            ; IntSet.iter
                (fun j ->
                  cooutputs.(n - 1).(j) <- IntSet.add i cooutputs.(n - 1).(j))
                outputs.(n).(i)
          done
        done
        ; let q =
            {
              max_dim= b.max_dim;
              size= new_size;
              inputs;
              outputs;
              coinputs;
              cooutputs;
            }
          in
          let quotient_map =
            Array.init dims (fun n -> Array.copy class_map.(n))
          in
          let quotient = Embedding.{ dom= b; cod= q; map= quotient_map } in
          let f' = Embedding.compose f quotient in
          let g' = Embedding.compose g quotient in
          (q, f', g')
