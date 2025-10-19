(** {1 Interpreter State} *)

(** {2 Core types} *)
type t

type type_entry = { data: Diagram.cell_data; complex: Complex.t }

(** {2 Constructors} *)
val empty : t

(** {2 Mutators} *)
val add_cell : t -> id:Id.Global.t -> Diagram.cell_data -> t

val add_type :
  t -> id:Id.Global.t -> data:Diagram.cell_data -> complex:Complex.t -> t

val update_type_complex : t -> id:Id.Global.t -> Complex.t -> t
val add_module : t -> id:Id.Module.t -> Complex.t -> t

(** {2 Lookups} *)
val find_cell : t -> Id.Global.t -> Diagram.cell_data option

val find_type : t -> Id.Global.t -> type_entry option
val find_module : t -> Id.Module.t -> Complex.t option
