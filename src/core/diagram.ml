type t = {
  shape: Ogposet.t;
  labels: Id.tag array array;
}

let shape d = d.shape
let labels d = d.labels
let dim d = Ogposet.dim d.shape
let is_round d = Ogposet.is_round d.shape
