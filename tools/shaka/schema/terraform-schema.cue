package terraform

// Terraform module schema. User CUE files in
// `<project>/cue/*.cue` declare a module conforming to this shape, and
// `shaka terraform emit` writes the corresponding HCL to
// `<project>/terraform/`.
//
// The schema models Terraform constructs structurally rather than as raw
// strings. HCL expressions that aren't JSON-representable (traversals like
// `var.region`, function calls like `sort(...)`, index access, conditionals,
// splats) are written as #Ref objects; the emitter parses the expression and
// produces the corresponding HCL AST node.

// #Ref expresses an arbitrary HCL expression — a traversal like
// `var.region`, a function call like `sort(var.x)`, index access like
// `var.list[0]`, a conditional, a splat, etc. The schema closes #Ref to
// exactly one field, `$ref`, so the emitter can detect refs without
// colliding with user-authored objects that happen to share field names.
// The `$ref` value is only required to be a non-empty string here; the
// emitter validates it by parsing it as an HCL expression (see
// `terraform/ir.rs`), rejecting anything that isn't well-formed HCL.
#Ref: {
	"$ref": string & !=""
}

// #NonRefObject: a generic object value that is *not* a ref. Without this,
// CUE's union semantics let an object carrying a `$ref` key satisfy the
// open object branch of #Value and bypass #Ref's single-field shape — the
// schema would accept it and only the IR walker would see it.
// Forbidding `$ref` here (`"$ref"?: _|_`) forces any object carrying a
// `$ref` key through #Ref, where the emitter then parses it.
#NonRefObject: {[string]: #Value, "$ref"?: _|_}

// #Value: the recursive value type for attributes. Anything that JSON can
// represent, plus refs.
#Value: bool | number | string | #Ref | [...#Value] | #NonRefObject

#Variable: {
	name:         string & =~"^[a-z][a-z0-9_]*$"
	description?: string & !=""
	// HCL type expression rendered verbatim: `string`, `number`, `bool`,
	// `list(string)`, `map(any)`, etc. Stored as a CUE string but emitted
	// as an HCL expression (not a string literal).
	type:       string & !=""
	default?:   #Value
	sensitive?: bool
}

#Output: {
	name:         string & =~"^[a-z][a-z0-9_]*$"
	value:        #Value
	description?: string & !=""
	sensitive?:   bool
}

#NestedBlock: {
	type: string & =~"^[a-z][a-z0-9_]*$"
	attributes?: {[string]: #Value}
	blocks?: [...#NestedBlock]
}

#Resource: {
	type: string & =~"^[a-z][a-z0-9_]*$"
	name: string & =~"^[a-zA-Z_][a-zA-Z0-9_]*$"
	attributes: {[string]: #Value}
	blocks?: [...#NestedBlock]
}

// HCL `data` blocks — provider-data lookups like
// `data "cloudflare_zone" "x" { ... }`. Shape mirrors #Resource; the
// emitter writes the `data` keyword instead of `resource` and the rest
// of the block is identical.
#DataSource: {
	type: string & =~"^[a-z][a-z0-9_]*$"
	name: string & =~"^[a-zA-Z_][a-zA-Z0-9_]*$"
	attributes: {[string]: #Value}
	blocks?: [...#NestedBlock]
}

#Provider: {
	name: string & =~"^[a-z][a-z0-9_]*$"
	attributes?: {[string]: #Value}
}

#RequiredProvider: {
	source:  string & !=""
	version: string & !=""
}

#TerraformBlock: {
	required_version?: string & !=""
	required_providers?: {[string]: #RequiredProvider}
}

// Top-level fields are the module. User files in `package terraform`
// contribute to these fields; CUE's unification handles merging across
// files.
terraform?: #TerraformBlock
providers?: [...#Provider]
variables?: [...#Variable]
data_sources?: [...#DataSource]
resources?: [...#Resource]
outputs?: [...#Output]
