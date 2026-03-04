(** {1 Morphisms} *)

(** {2 Core types} *)
type t

type cell_data = Diagram.cell_data

(** {2 Error-handling} *)
type 'a checked = 'a Error.checked

(** {2 Constructors} *)
val init : unit -> t checked

val extend :
  t ->
  tag:Id.Tag.t ->
  dim:int ->
  cell_data:cell_data ->
  image:Diagram.t ->
  t checked

val of_entries :
  (Id.Tag.t * int * cell_data * Diagram.t) list -> cellular:bool -> t

(** {2 Basic utilities} *)
val compose : t -> t -> t

val domain_of_definition : t -> Id.Tag.t list
val domain_in_dim : t -> int -> Id.Tag.t list
val dimensions : t -> int list
val domain_by_dim : t -> (int * Id.Tag.t list) list
val is_defined_at : t -> Id.Tag.t -> bool
val has_local_labels : t -> bool

(** {2 Destructor} *)
val apply : t -> Diagram.t -> Diagram.t checked

(** {2 Accessors} *)
val cell_data : t -> Id.Tag.t -> cell_data checked

val image : t -> Id.Tag.t -> Diagram.t checked
val dim : t -> Id.Tag.t -> int checked
val is_cellular : t -> bool
