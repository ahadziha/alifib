open Format

type mode = Interpret | Ast | Sexp

let usage = "Usage: alifib <input-file> [-o|--output <output-file>] [--ast]"
let read_file path = In_channel.with_open_bin path In_channel.input_all

let write_file path contents =
  Out_channel.with_open_bin path (fun oc ->
      Out_channel.output_string oc contents
      ; Out_channel.output_char oc '\n')

let set_output output ref_output =
  match !ref_output with
  | None ->
      ref_output := Some output
  | Some _ ->
      invalid_arg "Output file specified multiple times"

let parse_args () =
  let input = ref None in
  let output = ref None in
  let mode = ref Interpret in
  let speclist =
    [
      ( "-o",
        Arg.String (fun s -> set_output s output),
        "Write interpreter output to file" );
      ( "--output",
        Arg.String (fun s -> set_output s output),
        "Write interpreter output to file" );
      ( "--ast",
        Arg.Unit (fun () -> mode := Ast),
        "Pretty-print the AST instead of running the interpreter" );
      ( "--sexp",
        Arg.Unit (fun () -> mode := Sexp),
        "Give interpreter output as s-expression"
        );
    ]
  in
  let anon_fun arg =
    match !input with
    | None ->
        input := Some arg
    | Some _ ->
        raise (Arg.Bad "Multiple input files specified")
  in
  Arg.parse speclist anon_fun usage
  ; match !input with
    | None ->
        Arg.usage speclist usage ; exit 1
    | Some path ->
        (path, !output, !mode)

let has_error diagnostics =
  List.exists
    (fun (diag : Diagnostics.t) ->
      match diag.severity with `Error -> true | `Warning | `Info -> false)
    diagnostics

let print_or_write ?output contents =
  match output with
  | None ->
      print_endline contents
  | Some path ->
      write_file path contents

let run_ast input_path output_path =
  let source = Positions.Source.of_path input_path in
  let contents = read_file input_path in
  let stream = Token_stream.create ~source contents in
  let ast, diagnostics = Parser_driver.parse stream in
  if diagnostics <> [] then eprintf "%a@." Diagnostics.Report.pp diagnostics
  ; let rendered = Ast_pp.to_string ast in
    print_or_write ?output:output_path rendered
    ; if has_error diagnostics then exit 1

let run_interpreter ?(sexp=false) input_path output_path =
  let result = Session.run ~path:input_path () in
  let open Session in
  let { diagnostics; context; status } = result in
  let state = Interpreter.context_state context in
  let rendered =
    if sexp then
      Format.asprintf "%a" Sexplib.Sexp.pp_hum (State.sexp_of_t state)
    else Format.asprintf "%a" State.pp state
  in
  print_or_write ?output:output_path rendered
  ; if diagnostics <> [] then eprintf "%a@." Diagnostics.Report.pp diagnostics
  ; let has_diag_error = has_error diagnostics in
    let exit_needed =
      match status with
      | Success ->
          has_diag_error
      | Load_error | Parser_error | Interpreter_error ->
          true
    in
    if exit_needed then exit 1

let () =
  let input_path, output_path, mode = parse_args () in
  match mode with
  | Ast ->
      run_ast input_path output_path
  | Interpret ->
      run_interpreter input_path output_path
  | Sexp ->
      run_interpreter input_path output_path ~sexp:true
