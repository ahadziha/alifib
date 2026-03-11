(** {1 Interpreter State} *)

(** {2 Core types} *)
type t [@@deriving sexp_of]

type type_entry = { data: Diagram.cell_data; complex: Complex.t } [@@deriving sexp_of]
type cell_entry = { data: Diagram.cell_data; dim: int } [@@deriving sexp_of]

(** {2 Constructors} *)
val empty : t

(** {2 Mutators} *)
val set_cell : t -> id:Id.Global.t -> dim:int -> Diagram.cell_data -> t

val set_type :
  t -> id:Id.Global.t -> data:Diagram.cell_data -> complex:Complex.t -> t

val update_type_complex : t -> id:Id.Global.t -> Complex.t -> t
val set_module : t -> id:Id.Module.t -> Complex.t -> t

(** {2 Lookups} *)
val find_cell : t -> Id.Global.t -> cell_entry option

val find_type : t -> Id.Global.t -> type_entry option
val find_module : t -> Id.Module.t -> Complex.t option

(** {2 Pretty printing} *)
val pp : Format.formatter -> t -> unit
