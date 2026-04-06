(** High-level entry point that parses and interprets a source file. *)

type status = Load_error | Parser_error | Interpreter_error | Success

type result = {
  context: Interpreter.context;
  diagnostics: Diagnostics.report;
  status: status;
}

module Loader : sig
  type t = Interpreter.file_loader

  val make :
    ?search_paths:string list ->
    ?read_file:(string -> (string, Interpreter.load_error) Stdlib.result) ->
    unit ->
    t

  val default :
    ?search_paths:string list ->
    ?read_file:(string -> (string, Interpreter.load_error) Stdlib.result) ->
    unit ->
    t

  val with_search_paths : t -> string list -> t
  val prepend_search_paths : t -> string list -> t
  val append_search_paths : t -> string list -> t
end

val run : ?loader:Loader.t -> path:string -> unit -> result
