(** {1 Morphisms} *)

(** {2 Core types} *)
type t

(** {2 Error-handling} *)
type 'a checked = 'a Error.checked

(** {2 Basic utilities} *)
val domain_of_definition : t -> Id.Tag.t list

val is_defined_at : t -> Id.Tag.t -> bool

(** {2 Destructors} *)
val apply : t -> Diagram.t -> Diagram.t checked

val cell : t -> Id.Tag.t -> Diagram.t checked
val image : t -> Id.Tag.t -> Diagram.t checked
