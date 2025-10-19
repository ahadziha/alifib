(** {1 Complexes} *)

(** {2 Core types} *)
type t

type generator_entry = { tag: Id.Tag.t; dim: int }
type morphism_entry = { morphism: Morphism.t; domain: Id.Tag.t }
type local_cell_entry = { data: Diagram.cell_data; dim: int }

(** {2 Error-handling} *)
type 'a checked = 'a Error.checked

(** {2 Constructors} *)
val empty : t

val add_generator : t -> name:Id.Local.t -> dim:int -> tag:Id.Tag.t -> t checked
val add_diagram : t -> name:Id.Local.simple -> Diagram.t -> t checked

val add_morphism :
  t -> name:Id.Local.simple -> domain:Id.Tag.t -> Morphism.t -> t checked

val add_local_cell :
  t -> name:Id.Local.simple -> dim:int -> Diagram.cell_data -> t checked

(** {2 Lookups} *)
val find_generator : t -> Id.Local.t -> generator_entry option

val find_generator_by_tag : t -> Id.Tag.t -> Id.Local.t option
val generator_dim : t -> Id.Local.t -> int option
val generators_in_dim : t -> int -> Id.Local.t list
val find_diagram : t -> Id.Local.simple -> Diagram.t option
val find_morphism : t -> Id.Local.simple -> morphism_entry option
val find_local_cell : t -> Id.Local.simple -> local_cell_entry option
val local_cell_dim : t -> Id.Local.simple -> int option
val local_cells_in_dim : t -> int -> Id.Local.simple list

(** {2 Name utilities} *)
val name_in_use : t -> Id.Local.simple -> bool

val used_names : t -> Id.Local.simple list
