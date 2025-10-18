(** {1 Morphisms} *)

(** {2 Core types} *)
type t

type cell_data = { boundary_in: Diagram.t; boundary_out: Diagram.t }

(** {2 Error-handling} *)
type 'a checked = 'a Error.checked

(** {2 Constructors} *)
val init : unit -> t checked

val extend :
  t ->
  tag:Id.Tag.t ->
  dim:int ->
  boundary_in:Diagram.t ->
  boundary_out:Diagram.t ->
  image:Diagram.t ->
  t checked

(** {2 Basic utilities} *)
val domain_of_definition : t -> Id.Tag.t list

val is_defined_at : t -> Id.Tag.t -> bool

(** {2 Destructors} *)
val apply : t -> Diagram.t -> Diagram.t checked

val cell_data : t -> Id.Tag.t -> cell_data checked
val image : t -> Id.Tag.t -> Diagram.t checked
val dim : t -> Id.Tag.t -> int checked
val is_cellular : t -> bool
