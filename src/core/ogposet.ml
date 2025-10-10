(* Minimal oriented graded posets with embeddings. Internal: no well-formedness
   checks. Efficient and replaceable IntSet backend. *)

(* --- Replaceable integer set implementation --------------------------- *)

module IntSet = Set.Make (Int)
(* Swap this definition if desired; only this module needs to change. *)

type intset = IntSet.t

(* --- Core definitions ------------------------------------------------- *)

type sign = [ `Input | `Output | `Both ]
type grade = IntSet.t array
type adjacency = grade array

type t = {
  dim: int; (* stored for efficiency *)
  faces_in: adjacency;
  faces_out: adjacency;
  cofaces_in: adjacency;
  cofaces_out: adjacency;
}

type poset = t

let dim x = x.dim
let sizes x = Array.init (x.dim + 1) (fun d -> Array.length x.faces_in.(d))

let make ~dim ~faces_in ~faces_out ~cofaces_in ~cofaces_out =
  { dim; faces_in; faces_out; cofaces_in; cofaces_out }

(* --- Accessors -------------------------------------------------------- *)

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

(* --- Embeddings ------------------------------------------------------- *)

module Embedding = struct
  type t = { dom: poset; cod: poset; map: int array array }

  let make ~dom ~cod ~map = { dom; cod; map }
  let dom e = e.dom
  let cod e = e.cod
  let map e = e.map

  let id x =
    let sz = sizes x in
    let map = Array.mapi (fun _ n -> Array.init n (fun i -> i)) sz in
    { dom= x; cod= x; map }

  let compose f g =
    let map =
      Array.mapi
        (fun d arrf ->
          Array.init (Array.length arrf) (fun i ->
              let mid = arrf.(i) in
              g.map.(d).(mid)))
        f.map
    in
    { dom= f.dom; cod= g.cod; map }
end

(* --- Utility helpers -------------------------------------------------- *)

let set_map f s = IntSet.fold (fun x acc -> IntSet.add (f x) acc) s IntSet.empty

let set_filter_map f s =
  IntSet.fold
    (fun x acc -> match f x with None -> acc | Some y -> IntSet.add y acc)
    s IntSet.empty

let set_union_list = List.fold_left IntSet.union IntSet.empty

let ensure_dims x target =
  if x.dim + 1 = target then x
  else
    let grow adj =
      Array.init target (fun d ->
          if d < Array.length adj then Array.copy adj.(d) else [||])
    in
    {
      dim= target - 1;
      faces_in= grow x.faces_in;
      faces_out= grow x.faces_out;
      cofaces_in= grow x.cofaces_in;
      cofaces_out= grow x.cofaces_out;
    }

(* --- Disjoint union (coproduct object) -------------------------------- *)

type inj = { map: int array array }

let disjoint_union b c : t * inj * inj =
  let dims = max b.dim c.dim + 1 in
  let b = ensure_dims b dims and c = ensure_dims c dims in
  let szb = sizes b and szc = sizes c in
  let szu = Array.init dims (fun d -> szb.(d) + szc.(d)) in
  let inj_b = Array.mapi (fun _ n -> Array.init n (fun i -> i)) szb
  and inj_c =
    Array.mapi (fun d _ -> Array.init szc.(d) (fun i -> szb.(d) + i)) szb
  in
  let shift_down _ x = x
  and shift_up d x = if d = 0 then x else szb.(d - 1) + x
  and shift_up_co d x = szb.(d + 1) + x in
  let merge_faces fb fc =
    Array.init dims (fun d ->
        let n = szu.(d) in
        Array.init n (fun k ->
            if k < szb.(d) then set_map (shift_down d) fb.(d).(k)
            else set_map (shift_up d) fc.(d).(k - szb.(d))))
  and merge_cofaces cb cc =
    if dims = 0 then [||]
    else
      Array.init dims (fun d ->
          let n = szu.(d) in
          Array.init n (fun k ->
              if k < szb.(d) then set_map (shift_up_co d) cb.(d).(k)
              else set_map (shift_up_co d) cc.(d).(k - szb.(d))))
  in
  let u =
    {
      dim= dims - 1;
      faces_in= merge_faces b.faces_in c.faces_in;
      faces_out= merge_faces b.faces_out c.faces_out;
      cofaces_in= merge_cofaces b.cofaces_in c.cofaces_in;
      cofaces_out= merge_cofaces b.cofaces_out c.cofaces_out;
    }
  in
  (u, { map= inj_b }, { map= inj_c })

(* --- Union–find ------------------------------------------------------- *)

module UF = struct
  type t = { parent: int array }

  let make n = { parent= Array.init n (fun i -> i) }

  let rec find uf x =
    let p = uf.parent.(x) in
    if p = x then x
    else
      let r = find uf p in
      uf.parent.(x) <- r
      ; r

  let union uf a b =
    let ra = find uf a and rb = find uf b in
    if ra <> rb then uf.parent.(ra) <- rb
end

(* --- Quotient --------------------------------------------------------- *)

let quotient u ufs : t * int array array =
  let dims = u.dim + 1 in
  let sz_u = sizes u in
  let rep_to_new =
    Array.init dims (fun d ->
        let tbl = Hashtbl.create 16 and next = ref 0 in
        Array.init sz_u.(d) (fun i ->
            let r = UF.find ufs.(d) i in
            match Hashtbl.find_opt tbl r with
            | Some k ->
                k
            | None ->
                let k = !next in
                incr next ; Hashtbl.add tbl r k ; k))
  in
  let sz_q =
    Array.init dims (fun d ->
        let maxk = ref (-1) in
        Array.iter (fun k -> if k > !maxk then maxk := k) rep_to_new.(d)
        ; !maxk + 1)
  in
  let classes =
    Array.init dims (fun d ->
        let buckets = Array.init sz_q.(d) (fun _ -> ref []) in
        for i = 0 to sz_u.(d) - 1 do
          let k = rep_to_new.(d).(i) in
          buckets.(k) := i :: !(buckets.(k))
        done
        ; Array.init sz_q.(d) (fun k -> List.rev !(buckets.(k))))
  in
  let rebuild_faces src =
    Array.init dims (fun d ->
        Array.init sz_q.(d) (fun k ->
            let olds = classes.(d).(k) in
            let mapped =
              List.map
                (fun i ->
                  if d = 0 then IntSet.empty
                  else set_map (fun x -> rep_to_new.(d - 1).(x)) src.(d).(i))
                olds
            in
            set_union_list mapped))
  and rebuild_cofaces src =
    if dims = 0 then [||]
    else
      Array.init dims (fun d ->
          Array.init sz_q.(d) (fun k ->
              let olds = classes.(d).(k) in
              let mapped =
                List.map
                  (fun i ->
                    if d = dims - 1 then IntSet.empty
                    else set_map (fun x -> rep_to_new.(d + 1).(x)) src.(d).(i))
                  olds
              in
              set_union_list mapped))
  in
  let q =
    {
      dim= dims - 1;
      faces_in= rebuild_faces u.faces_in;
      faces_out= rebuild_faces u.faces_out;
      cofaces_in= rebuild_cofaces u.cofaces_in;
      cofaces_out= rebuild_cofaces u.cofaces_out;
    }
  in
  (q, rep_to_new)

(* --- Universal constructions ----------------------------------------- *)

type coproduct = { sum: t; inl: Embedding.t; inr: Embedding.t }

let coproduct b c =
  let u, inj_b, inj_c = disjoint_union b c in
  let inl = Embedding.make ~dom:b ~cod:u ~map:inj_b.map
  and inr = Embedding.make ~dom:c ~cod:u ~map:inj_c.map in
  { sum= u; inl; inr }

type pushout = { po: t; leg1: Embedding.t; leg2: Embedding.t }

let pushout i j =
  let a = Embedding.dom i and b = Embedding.cod i and c = Embedding.cod j in
  let u, inj_b, inj_c = disjoint_union b c in
  let dims = max a.dim (max b.dim c.dim) + 1 in
  let ufs = Array.init dims (fun d -> UF.make (Array.length u.faces_in.(d))) in
  let sz_a = sizes a in
  for d = 0 to Array.length sz_a - 1 do
    for x = 0 to sz_a.(d) - 1 do
      let b_ix = i.Embedding.map.(d).(x) and c_ix = j.Embedding.map.(d).(x) in
      let u_b = inj_b.map.(d).(b_ix) and u_c = inj_c.map.(d).(c_ix) in
      UF.union ufs.(d) u_b u_c
    done
  done
  ; let p, qmap = quotient u ufs in
    let map_b =
      Array.mapi (fun d arr -> Array.map (fun i -> qmap.(d).(i)) arr) inj_b.map
    and map_c =
      Array.mapi (fun d arr -> Array.map (fun i -> qmap.(d).(i)) arr) inj_c.map
    in
    let leg1 = Embedding.make ~dom:b ~cod:p ~map:map_b
    and leg2 = Embedding.make ~dom:c ~cod:p ~map:map_c in
    { po= p; leg1; leg2 }

type coequaliser = { coeq: t; emb: Embedding.t }

let coequaliser f g =
  let a = Embedding.dom f and b = Embedding.cod f in
  let dims = max a.dim b.dim + 1 in
  let ufs = Array.init dims (fun d -> UF.make (Array.length b.faces_in.(d))) in
  let sz_a = sizes a in
  for d = 0 to Array.length sz_a - 1 do
    for x = 0 to sz_a.(d) - 1 do
      UF.union ufs.(d) f.Embedding.map.(d).(x) g.Embedding.map.(d).(x)
    done
  done
  ; let q, qmap = quotient b ufs in
    let map_a =
      Array.mapi
        (fun d arrf -> Array.map (fun i -> qmap.(d).(i)) arrf)
        f.Embedding.map
    in
    { coeq= q; emb= Embedding.make ~dom:a ~cod:q ~map:map_a }

(* --- Boundary --------------------------------------------------------- *)

let boundary (sign : sign) (at_dim : int) (x : t) : t * Embedding.t =
  let dmax = x.dim in
  if dmax <= 0 then
    let e =
      {
        dim= -1;
        faces_in= [||];
        faces_out= [||];
        cofaces_in= [||];
        cofaces_out= [||];
      }
    in
    (e, Embedding.id e)
  else
    let dims = dmax + 1 in
    (* 1) select at_dim-cells with no cofaces of given sign *)
    let keep = Array.init dims (fun _ -> IntSet.empty) in
    let n_d = Array.length x.faces_in.(at_dim) in
    for i = 0 to n_d - 1 do
      if IntSet.is_empty (cofaces_of sign x ~dim:at_dim ~pos:i) then
        keep.(at_dim) <- IntSet.add i keep.(at_dim)
    done
    ; (* 2) downward closure via both face orientations *)
      let stack =
        ref (List.map (fun i -> (at_dim, i)) (IntSet.elements keep.(at_dim)))
      in
      while !stack <> [] do
        match !stack with
        | [] ->
            ()
        | (d, i) :: rest ->
            stack := rest
            ; if d > 0 then
                IntSet.iter
                  (fun j ->
                    if not (IntSet.mem j keep.(d - 1)) then (
                      keep.(d - 1) <- IntSet.add j keep.(d - 1)
                      ; stack := (d - 1, j) :: !stack))
                  (faces_of `Both x ~dim:d ~pos:i)
      done
      ; (* 3) build index maps old -> new, and inverse new -> old *)
        let new_index =
          Array.mapi
            (fun d _ ->
              let n = Array.length x.faces_in.(d) in
              let a = Array.make n (-1) and k = ref 0 in
              IntSet.iter
                (fun old ->
                  a.(old) <- !k
                  ; incr k)
                keep.(d)
              ; a)
            x.faces_in
        in
        let sz_sub = Array.map IntSet.cardinal keep in
        let old_of_new =
          Array.mapi
            (fun d _ ->
              let inv = Array.make sz_sub.(d) 0 in
              IntSet.iter
                (fun old ->
                  let ni = new_index.(d).(old) in
                  if ni >= 0 then inv.(ni) <- old)
                keep.(d)
              ; inv)
            x.faces_in
        in
        let remap_lower src dd =
          set_filter_map
            (fun j ->
              let jj = new_index.(dd - 1).(j) in
              if jj = -1 then None else Some jj)
            src
        and remap_upper src dd =
          set_filter_map
            (fun j ->
              let jj = new_index.(dd + 1).(j) in
              if jj = -1 then None else Some jj)
            src
        in
        (* 4) rebuild induced adjacency *)
        let faces_in' =
          Array.mapi
            (fun dd _ ->
              Array.init sz_sub.(dd) (fun new_i ->
                  if dd = 0 then IntSet.empty
                  else
                    let old_i = old_of_new.(dd).(new_i) in
                    remap_lower (faces_of `Input x ~dim:dd ~pos:old_i) dd))
            x.faces_in
        in
        let faces_out' =
          Array.mapi
            (fun dd _ ->
              Array.init sz_sub.(dd) (fun new_i ->
                  if dd = 0 then IntSet.empty
                  else
                    let old_i = old_of_new.(dd).(new_i) in
                    remap_lower (faces_of `Output x ~dim:dd ~pos:old_i) dd))
            x.faces_out
        in
        let cofaces_in' =
          Array.mapi
            (fun dd _ ->
              Array.init sz_sub.(dd) (fun new_i ->
                  if dd = dmax then IntSet.empty
                  else
                    let old_i = old_of_new.(dd).(new_i) in
                    remap_upper (cofaces_of `Input x ~dim:dd ~pos:old_i) dd))
            x.cofaces_in
        in
        let cofaces_out' =
          Array.mapi
            (fun dd _ ->
              Array.init sz_sub.(dd) (fun new_i ->
                  if dd = dmax then IntSet.empty
                  else
                    let old_i = old_of_new.(dd).(new_i) in
                    remap_upper (cofaces_of `Output x ~dim:dd ~pos:old_i) dd))
            x.cofaces_out
        in
        let sub =
          {
            dim= dmax;
            faces_in= faces_in';
            faces_out= faces_out';
            cofaces_in= cofaces_in';
            cofaces_out= cofaces_out';
          }
        in
        (* inclusion embedding: sub -> x *)
        let map_incl =
          Array.mapi
            (fun dd _ ->
              Array.init sz_sub.(dd) (fun new_i -> old_of_new.(dd).(new_i)))
            sub.faces_in
        in
        (sub, Embedding.make ~dom:sub ~cod:x ~map:map_incl)

let boundary_top (s : sign) (x : t) : t * Embedding.t = boundary s (dim x - 1) x
