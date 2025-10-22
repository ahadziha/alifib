type span = Positions.span
type 'a node = { node: 'a; span: span option }
module Nonempty : sig
  type 'a t = { head : 'a; tail : 'a list }

  val singleton : 'a -> 'a t
  val append : 'a t -> 'a -> 'a t
  val to_list : 'a t -> 'a list
  val map : ('a -> 'b) -> 'a t -> 'b t
end

type 'a nonempty = 'a Nonempty.t
type identifier = string node
type nat = string node
type location = identifier nonempty

type program = { blocks: block node list }

and block =
  | Type_block of type_block
  | Builder_block of local_block

and type_block = { type_instructions: cpx_instr_type node list }

and local_block = {
  location: cpx_builder node;
  instructions: cpx_instr_local node list;
}

and cpx_builder = {
  root: location node option;
  extension: cpx_instr node list;
}

and cpx_block = cpx_instr node list

and cpx_instr_type =
  | Generator_type of generator_type
  | Diagram_namer_type of diagram_namer
  | Morphism_namer_type of morphism_namer
  | Include_module of include_statement

and cpx_instr =
  | Generator of generator
  | Diagram_namer of diagram_namer
  | Morphism_namer of morphism_namer
  | Include of include_statement
  | Attach of attach_statement

and cpx_instr_local =
  | Diagram_namer_local of diagram_namer
  | Morphism_namer_local of morphism_namer
  | Assert of assert_statement

and generator_type = { generator: generator node; value: cpx_builder node }
and generator = { name: identifier; boundaries: boundaries option }
and boundaries = { input: diagram node; output: diagram node }

and diagram_namer = {
  diagram_name: identifier;
  constraints: boundaries option;
  diagram_def: diagram node;
}

and morphism_namer = {
  morphism_name: identifier;
  domain: location node;
  morphism_def: morphism node;
}

and include_statement = {
  inclusion: identifier option;
  include_location: location node;
}

and attach_statement = {
  attachment: identifier;
  shape: location node;
  along: morphism node option;
}

and assert_statement = { left: diagram node; right: diagram node }

and diagram =
  | Diagram_concat of diagram_component node nonempty
  | Diagram_paste of {
      paste_base: diagram node;
      paste_dim: nat;
      paste_suffix: diagram_component node nonempty;
    }

and diagram_component = {
  prefix: morphism node option;
  base: diagram_simple node;
  suffix: boundary_selector node list;
}

and diagram_simple =
  | Diagram_name of identifier
  | Diagram_parens of diagram node
  | Diagram_hole

and boundary_selector = In | Out
and morphism = morphism_expr node nonempty

and morphism_expr =
  | Morphism_builder of morphism_builder
  | Morphism_with_domain of {
      builder: morphism_builder node;
      domain: cpx_builder node;
    }

and morphism_builder =
  | Morphism_simple of morphism_simple
  | Morphism_block of {
      base: morphism_simple node option;
      extension: morphism_block;
    }

and morphism_block = morphism_instr node list

and morphism_instr =
  | From_cell of diagram node * diagram node
  | From_morphism of morphism node * morphism node

and morphism_simple =
  | Morphism_name of identifier
  | Morphism_parens of morphism node

val node : ?span:span -> 'a -> 'a node
val empty : program
val of_tokens : Token.kind list -> program
