(* ogposet.mli — interface for oriented graded posets, IntSet pluggable by redefining module IntSet *)

(** Elements (opaque). *)
type elt

(** The main OGPoset type (opaque). *)
type t
type poset = t

(** Graded subsets. *)
module Sub : sig
  type t
  val empty : poset -> t
  val of_list : poset -> elt list -> t
end

(** Embeddings between OGPosets. *)
module Embedding : sig
  type t
  val dom : t -> poset
  val cod : t -> poset
end

(** Constructors and modifications. *)
val empty : t
val add0 : t -> int -> t * elt list
val addN : t -> dim:int -> inputs:Sub.t list -> outputs:Sub.t list -> t * elt list

(** Accessors. *)
val inputs : t -> elt -> Sub.t
val outputs : t -> elt -> Sub.t
val coinputs : t -> elt -> Sub.t
val cooutputs : t -> elt -> Sub.t

(** Derived structures and operations. *)
val closure : t -> Sub.t -> Sub.t
val embed : t -> Sub.t -> t * Embedding.t
val bd_in : t -> int -> Sub.t
val bd_out : t -> int -> Sub.t

(** Fast asymmetric pushout: attaches the complement of shared part. *)
val attach : Embedding.t -> Embedding.t -> t * Embedding.t * Embedding.t

(** Heuristic pushout: automatically choose the cheaper attachment direction. *)
val pushout : Embedding.t -> Embedding.t -> t * Embedding.t * Embedding.t
