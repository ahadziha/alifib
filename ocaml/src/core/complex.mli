(** {1 Complexes} *)

(** {2 Core types} *)
type t

type generator_entry = { tag: Id.Tag.t; dim: int }
type morphism_domain = Type of Id.Global.t | Module of Id.Module.t
type morphism_entry = { morphism: Morphism.t; domain: morphism_domain }
type local_cell_entry = { data: Diagram.cell_data; dim: int }

(** {2 Constructors} *)
val empty : t

val add_generator : t -> name:Id.Local.t -> classifier:Diagram.t -> t
val add_diagram : t -> name:Id.Local.t -> Diagram.t -> t

val add_morphism :
  t -> name:Id.Local.t -> domain:morphism_domain -> Morphism.t -> t

val add_local_cell : t -> name:Id.Local.t -> dim:int -> Diagram.cell_data -> t

(** {2 Lookups} *)
val find_generator : t -> Id.Local.t -> generator_entry option

val find_generator_by_tag : t -> Id.Tag.t -> Id.Local.t option
val classifier : t -> Id.Local.t -> Diagram.t option
val generator_dim : t -> Id.Local.t -> int option
val generators_in_dim : t -> int -> Id.Local.t list
val find_diagram : t -> Id.Local.t -> Diagram.t option
val find_morphism : t -> Id.Local.t -> morphism_entry option
val find_local_cell : t -> Id.Local.t -> local_cell_entry option
val local_cell_dim : t -> Id.Local.t -> int option
val local_cells_in_dim : t -> int -> Id.Local.t list

(** {2 Name utilities} *)
val generator_names : t -> Id.Local.t list

val diagram_names : t -> Id.Local.t list
val morphism_names : t -> Id.Local.t list
val name_in_use : t -> Id.Local.t -> bool
val used_names : t -> Id.Local.t list
