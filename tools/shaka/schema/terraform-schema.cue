package terraform

// Terraform module schema. User CUE files in
// `<project>/cue/*.cue` declare a module conforming to this shape, and
// `shaka terraform emit` writes the corresponding HCL to
// `<project>/terraform/`.
//
// The schema models Terraform constructs structurally rather than as raw
// strings. HCL expressions that aren't JSON-representable (traversals like
// `var.region` or `linode_instance.devbox.id`) are written as #Ref
// objects; the emitter recognizes them and produces HCL Traversal nodes.

// #Ref expresses an HCL traversal — a dotted chain of identifiers like
// `var.region` or `linode_instance.devbox.id`. The schema closes #Ref to
// exactly one field, `$ref`, so the emitter can detect refs without
// colliding with user-authored objects that happen to share field names.
#Ref: {
	"$ref": string & =~"^[a-zA-Z_][a-zA-Z0-9_]*(\\.[a-zA-Z_][a-zA-Z0-9_]*)*$"
}

// #Value: the recursive value type for attributes. Anything that JSON can
// represent, plus refs.
#Value: bool | number | string | #Ref | [...#Value] | {[string]: #Value}

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
