(* ogposet.ml — implementation for oriented graded posets with pluggable IntSet backend *)

(* You may redefine IntSet below to use another integer-set backend, e.g. a bitset or hybrid. *)
module IntSet = Set.Make(Int)

(* --- Types --- *)

type elt = { dim : int; pos : int }

type faces = IntSet.t array array

type t = {
  max_dim : int;
  size : int array;
  inputs : faces;
  outputs : faces;
  coinputs : faces;
  cooutputs : faces;
}

type poset = t

module Sub = struct
  module IntMap = Map.Make(Int)

  type t = {
    dims : IntSet.t;
    cells : IntSet.t IntMap.t;
  }

  let empty (_:poset) = { dims = IntSet.empty; cells = IntMap.empty }

  let dims (s:t) = s.dims

  let lookup dim cells =
    try Some (IntMap.find dim cells) with Not_found -> None

  let ensure_nonnegative ~dim ~pos =
    if dim < 0 then invalid_arg "Sub: negative dimension"
    else if pos < 0 then invalid_arg "Sub: negative position"

  let positions (s:t) ~dim =
    match lookup dim s.cells with
    | Some set -> set
    | None -> IntSet.empty

  let add_position (s:t) ~dim ~pos =
    ensure_nonnegative ~dim ~pos;
    let updated =
      match lookup dim s.cells with
      | None -> IntSet.singleton pos
      | Some set -> IntSet.add pos set
    in
    { dims = IntSet.add dim s.dims; cells = IntMap.add dim updated s.cells }

  let add_positions (s:t) ~dim (positions_set:IntSet.t) =
    if dim < 0 then invalid_arg "Sub: negative dimension";
    if IntSet.is_empty positions_set then s
    else (
      IntSet.iter (fun pos -> if pos < 0 then invalid_arg "Sub: negative position") positions_set;
      let updated =
        match lookup dim s.cells with
        | None -> positions_set
        | Some set -> IntSet.union positions_set set
      in
      { dims = IntSet.add dim s.dims; cells = IntMap.add dim updated s.cells }
    )

  let mem_position (s:t) ~dim ~pos =
    match lookup dim s.cells with
    | Some set -> IntSet.mem pos set
    | None -> false

  let iter (s:t) ~f =
    IntSet.iter (fun dim ->
      match lookup dim s.cells with
      | Some set -> IntSet.iter (fun pos -> f ~dim ~pos) set
      | None -> ()
    ) s.dims

  let validate_bounds (g:poset) ~dim ~pos =
    if dim < 0 || dim > g.max_dim then invalid_arg "Sub: dimension out of bounds";
    if pos < 0 || pos >= g.size.(dim) then invalid_arg "Sub: position out of bounds"

  let of_list (g:poset) (els:elt list) : t =
    List.fold_left (fun acc e ->
      validate_bounds g ~dim:e.dim ~pos:e.pos;
      add_position acc ~dim:e.dim ~pos:e.pos
    ) (empty g) els

  let of_dim_set (g:poset) ~dim (set:IntSet.t) : t =
    if IntSet.is_empty set then empty g
    else (
      IntSet.iter (fun pos -> validate_bounds g ~dim ~pos) set;
      { dims = IntSet.singleton dim; cells = IntMap.add dim set IntMap.empty }
    )
end

module Embedding = struct
  type t = { dom : t; cod : t; map : int array array }
end

(* --- Helpers --- *)
let dim_count g = g.max_dim + 1
let size_in g n = if n < 0 || n > g.max_dim then 0 else g.size.(n)

(* --- Construction with incremental coface maintenance --- *)

let empty : t = {
  max_dim = -1;
  size = [||];
  inputs = [||]; outputs = [||]; coinputs = [||]; cooutputs = [||];
}

let extend_to_dim (g:t) (n:int) : t =
  if n <= g.max_dim then g else
  let new_dims = n + 1 in
  let grow_faces (a:faces) =
    let arr = Array.make new_dims [||] in
    Array.blit a 0 arr 0 (Array.length a); arr
  in
  let size' = Array.make new_dims 0 in
  Array.blit g.size 0 size' 0 (Array.length g.size);
  { max_dim = n; size = size';
    inputs = grow_faces g.inputs; outputs = grow_faces g.outputs;
    coinputs = grow_faces g.coinputs; cooutputs = grow_faces g.cooutputs }

let ensure_layer_capacity arr new_len =
  if Array.length arr < new_len then (
    let a = Array.make new_len IntSet.empty in
    Array.blit arr 0 a 0 (Array.length arr); a
  ) else arr

let add0 (g:t) (m:int) : t * elt list =
  if m < 0 then invalid_arg "add0: negative count";
  let g = extend_to_dim g 0 in
  let old = if g.max_dim < 0 then 0 else g.size.(0) in
  let size' = Array.copy g.size in
  size'.(0) <- old + m;
  let coinputs' = Array.copy g.coinputs in
  let cooutputs' = Array.copy g.cooutputs in
  if Array.length coinputs'.(0) < size'.(0) then coinputs'.(0) <- ensure_layer_capacity coinputs'.(0) size'.(0);
  if Array.length cooutputs'.(0) < size'.(0) then cooutputs'.(0) <- ensure_layer_capacity cooutputs'.(0) size'.(0);
  let g' = { g with max_dim = max g.max_dim 0; size = size'; coinputs = coinputs'; cooutputs = cooutputs' } in
  let elts = List.init m (fun i -> { dim = 0; pos = old + i }) in
  g', elts

let addN (g:t) ~(dim:int) ~(inputs:Sub.t list) ~(outputs:Sub.t list) : t * elt list =
  if dim <= 0 then invalid_arg "addN: dim must be > 0";
  if List.length inputs <> List.length outputs then invalid_arg "addN: length mismatch";
  let g = extend_to_dim g dim in
  let n_old = size_in g dim in
  let k = List.length inputs in
  let n_new = n_old + k in
  let size' = Array.copy g.size in
  size'.(dim) <- n_new;
  let inputs' = Array.copy g.inputs in
  let outputs' = Array.copy g.outputs in
  if dim > 0 then (
    inputs'.(dim) <- ensure_layer_capacity inputs'.(dim) n_new;
    outputs'.(dim) <- ensure_layer_capacity outputs'.(dim) n_new
  );
  let coinputs' = Array.copy g.coinputs in
  let cooutputs' = Array.copy g.cooutputs in
  if dim-1 >= 0 then (
    coinputs'.(dim-1) <- ensure_layer_capacity coinputs'.(dim-1) (size_in g (dim-1));
    cooutputs'.(dim-1) <- ensure_layer_capacity cooutputs'.(dim-1) (size_in g (dim-1))
  );
  let allowed_dim = dim - 1 in
  let validate_faces kind (sub:Sub.t) =
    let dims = Sub.dims sub in
    let invalid = ref false in
    IntSet.iter (fun d -> if d <> allowed_dim then invalid := true) dims;
    if !invalid then
      invalid_arg ("addN: " ^ kind ^ " subset must lie entirely in dimension " ^ string_of_int allowed_dim);
    let set = Sub.positions sub ~dim:allowed_dim in
    IntSet.iter (fun j ->
      if j < 0 || j >= size_in g allowed_dim then
        invalid_arg ("addN: " ^ kind ^ " subset references out-of-range position")
    ) set;
    set
  in
  List.iteri (fun i sub ->
    let set = validate_faces "inputs" sub in
    inputs'.(dim).(n_old + i) <- set;
    IntSet.iter (fun j ->
      coinputs'.(allowed_dim).(j) <- IntSet.add (n_old + i) coinputs'.(allowed_dim).(j)
    ) set
  ) inputs;
  List.iteri (fun i sub ->
    let set = validate_faces "outputs" sub in
    outputs'.(dim).(n_old + i) <- set;
    IntSet.iter (fun j ->
      cooutputs'.(allowed_dim).(j) <- IntSet.add (n_old + i) cooutputs'.(allowed_dim).(j)
    ) set
  ) outputs;
  let g' = { g with size = size'; inputs = inputs'; outputs = outputs'; coinputs = coinputs'; cooutputs = cooutputs' } in
  let elts = List.init k (fun i -> { dim; pos = n_old + i }) in
  g', elts

(* --- Accessors --- *)

let get_faces (g:t) arr (e:elt) : Sub.t =
  if e.dim = 0 then Sub.empty g
  else Sub.of_dim_set g ~dim:(e.dim - 1) arr.(e.dim).(e.pos)

let get_cofaces (g:t) arr (e:elt) : Sub.t =
  let next_dim = e.dim + 1 in
  if next_dim > g.max_dim then Sub.empty g
  else Sub.of_dim_set g ~dim:next_dim arr.(e.dim).(e.pos)

let inputs g e = get_faces g g.inputs e
let outputs g e = get_faces g g.outputs e
let coinputs g e = if e.dim >= g.max_dim then Sub.empty g else get_cofaces g g.coinputs e
let cooutputs g e = if e.dim >= g.max_dim then Sub.empty g else get_cofaces g g.cooutputs e

(* --- Closure (downward) --- *)

let closure (g:t) (s:Sub.t) : Sub.t =
  if g.max_dim < 0 then s else
  let q = Array.init (dim_count g) (fun _ -> Queue.create ()) in
  Sub.iter s ~f:(fun ~dim ~pos ->
    if dim < 0 || dim > g.max_dim then invalid_arg "closure: subset dimension out of bounds";
    Queue.add pos q.(dim)
  );
  let res = ref s in
  for n = g.max_dim downto 0 do
    while not (Queue.is_empty q.(n)) do
      let i = Queue.take q.(n) in
      if n > 0 then (
        let add_faces faces =
          IntSet.iter (fun j ->
            if not (Sub.mem_position !res ~dim:(n-1) ~pos:j) then (
              res := Sub.add_position !res ~dim:(n-1) ~pos:j;
              Queue.add j q.(n-1)
            )
          ) faces.(n).(i)
        in
        add_faces g.inputs;
        add_faces g.outputs
      )
    done
  done;
  !res

(* --- Embed --- *)

let embed (g:t) (s:Sub.t) : t * Embedding.t =
  let s_cl = closure g s in
  for n=0 to g.max_dim do
    if IntSet.cardinal (Sub.positions s ~dim:n) <> IntSet.cardinal (Sub.positions s_cl ~dim:n)
    then invalid_arg "embed: subset not closed"
  done;
  let dims = dim_count g in
  let map = Array.init dims (fun n -> Array.make g.size.(n) (-1)) in
  let size' = Array.make dims 0 in
  for n=0 to g.max_dim do
    let elements = IntSet.elements (Sub.positions s ~dim:n) in
    size'.(n) <- List.length elements;
    List.iteri (fun idx i_old -> map.(n).(i_old) <- idx) elements
  done;
  let inputs' = Array.init dims (fun n -> if n=0 then [||] else Array.make size'.(n) IntSet.empty) in
  let outputs' = Array.init dims (fun n -> if n=0 then [||] else Array.make size'.(n) IntSet.empty) in
  let remap_faces n set =
    IntSet.fold (fun j acc ->
      let mapped = map.(n-1).(j) in
      if mapped < 0 then invalid_arg "embed: subset not closed";
      IntSet.add mapped acc
    ) set IntSet.empty
  in
  for n=1 to g.max_dim do
    let elements = IntSet.elements (Sub.positions s ~dim:n) in
    List.iteri (fun idx i_old ->
      inputs'.(n).(idx) <- remap_faces n g.inputs.(n).(i_old);
      outputs'.(n).(idx) <- remap_faces n g.outputs.(n).(i_old)
    ) elements
  done;
  let coinputs' = Array.init dims (fun n -> Array.make size'.(n) IntSet.empty) in
  let cooutputs' = Array.init dims (fun n -> Array.make size'.(n) IntSet.empty) in
  for n=1 to g.max_dim do
    for i=0 to size'.(n)-1 do
      IntSet.iter (fun j -> coinputs'.(n-1).(j) <- IntSet.add i coinputs'.(n-1).(j)) inputs'.(n).(i);
      IntSet.iter (fun j -> cooutputs'.(n-1).(j) <- IntSet.add i cooutputs'.(n-1).(j)) outputs'.(n).(i)
    done
  done;
  let h = { max_dim = g.max_dim; size = size'; inputs = inputs'; outputs = outputs'; coinputs = coinputs'; cooutputs = cooutputs' } in
  let emb = Embedding.{ dom = h; cod = g; map } in
  h, emb

(* --- Boundaries --- *)

let has_any_coface g n i =
  if n < g.max_dim then not (IntSet.is_empty g.coinputs.(n).(i) && IntSet.is_empty g.cooutputs.(n).(i)) else false

let is_input_face g k i = if k < g.max_dim then IntSet.is_empty g.cooutputs.(k).(i) else true
let is_output_face g k i = if k < g.max_dim then IntSet.is_empty g.coinputs.(k).(i) else true

let bd_in (g:t) (k:int) : Sub.t =
  if k < 0 then invalid_arg "bd_in: negative dimension";
  if k > g.max_dim then Sub.empty g
  else
    let subset = ref (Sub.empty g) in
    for i=0 to g.size.(k)-1 do
      if is_input_face g k i then subset := Sub.add_position !subset ~dim:k ~pos:i
    done;
    for j=0 to k-1 do
      for i=0 to g.size.(j)-1 do
        if not (has_any_coface g j i) then subset := Sub.add_position !subset ~dim:j ~pos:i
      done
    done;
    closure g !subset

let bd_out (g:t) (k:int) : Sub.t =
  if k < 0 then invalid_arg "bd_out: negative dimension";
  if k > g.max_dim then Sub.empty g
  else
    let subset = ref (Sub.empty g) in
    for i=0 to g.size.(k)-1 do
      if is_output_face g k i then subset := Sub.add_position !subset ~dim:k ~pos:i
    done;
    for j=0 to k-1 do
      for i=0 to g.size.(j)-1 do
        if not (has_any_coface g j i) then subset := Sub.add_position !subset ~dim:j ~pos:i
      done
    done;
    closure g !subset

(* --- Fast asymmetric pushout: attach complement of g(C) in B onto A --- *)

let attach (f:Embedding.t) (g:Embedding.t) : t * Embedding.t * Embedding.t =
  if f.dom != g.dom then invalid_arg "attach: embeddings must share domain";
  let A = f.cod and B = g.cod and C = f.dom in
  let sizeA n = if n <= A.max_dim then A.size.(n) else 0 in
  let sizeB n = if n <= B.max_dim then B.size.(n) else 0 in
  let dims = max (A.max_dim + 1) (B.max_dim + 1) in
  let inv_g = Array.init dims (fun n -> Array.make (sizeB n) (-1)) in
  for n=0 to C.max_dim do
    for i=0 to C.size.(n)-1 do
      let jB = g.map.(n).(i) in
      inv_g.(n).(jB) <- i
    done
  done;
  let P = ref A in
  let mapB_to_P = Array.init dims (fun n -> Array.make (sizeB n) (-1)) in
  for n=0 to B.max_dim do
    let comp = ref [] in
    for j=0 to sizeB n - 1 do if inv_g.(n).(j) < 0 then comp := j :: !comp done;
    if !comp <> [] then (
      if n = 0 then (
        let count = List.length !comp in
        let p_after, new_elts = add0 !P count in
        P := p_after;
        List.iter2 (fun j e -> mapB_to_P.(0).(j) <- e.pos) (List.rev !comp) new_elts
      ) else (
        let in_subs = ref [] and out_subs = ref [] in
        let order = List.rev !comp in
        List.iter (fun j ->
          let map_face set =
            IntSet.fold (fun k acc ->
              if k < sizeB (n-1) && inv_g.(n-1).(k) >= 0 then (
                let c = inv_g.(n-1).(k) in
                let a_pos = f.map.(n-1).(c) in
                Sub.add_position acc ~dim:(n-1) ~pos:a_pos
              ) else (
                let ppos = mapB_to_P.(n-1).(k) in
                if ppos < 0 then invalid_arg "attach: lower-dim complement not yet added";
                Sub.add_position acc ~dim:(n-1) ~pos:ppos
              )
            ) set (Sub.empty !P)
          in
          in_subs := map_face B.inputs.(n).(j) :: !in_subs;
          out_subs := map_face B.outputs.(n).(j) :: !out_subs
        ) order;
        let p_after, new_elts = addN !P ~dim:n ~inputs:(List.rev !in_subs) ~outputs:(List.rev !out_subs) in
        P := p_after;
        List.iter2 (fun j e -> mapB_to_P.(n).(j) <- e.pos) order new_elts
      )
    )
  done;
  let P_final = !P in
  let mapA = Array.init dims (fun n -> Array.init (sizeA n) (fun i -> i)) in
  let mapB = Array.init dims (fun n ->
    Array.init (sizeB n) (fun j ->
      if j < sizeB n && inv_g.(n).(j) >= 0 then
        let c = inv_g.(n).(j) in
        if n <= A.max_dim then f.map.(n).(c) else invalid_arg "attach: dim mismatch"
      else mapB_to_P.(n).(j)
    ))
  in
  let iotaA = Embedding.{ dom = A; cod = P_final; map = mapA } in
  let iotaB = Embedding.{ dom = B; cod = P_final; map = mapB } in
  (P_final, iotaA, iotaB)

(* --- Heuristic pushout: choose cheaper direction --- *)

let pushout (f:Embedding.t) (g:Embedding.t) : t * Embedding.t * Embedding.t =
  if f.dom != g.dom then invalid_arg "pushout: embeddings must share domain";
  let A = f.cod and B = g.cod and C = f.dom in
  let size_total x =
    let s = ref 0 in
    for n=0 to x.max_dim do s := !s + x.size.(n) done; !s
  in
  let a_tot = size_total A and b_tot = size_total B and c_tot = size_total C in
  let add_B_into_A = b_tot - c_tot in
  let add_A_into_B = a_tot - c_tot in
  if add_B_into_A <= add_A_into_B then
    attach f g
  else
    let p, iB, iA = attach g f in
    (p, iA, iB)
    
