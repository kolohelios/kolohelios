// Minimal fixture demonstrating the `#Area` shape. Personal data —
// the actual areas-of-focus tree — lives in `kolohelios/personal-os`
// (#620), not here. This file exists so `cue vet` has something to
// validate against the schema locally.
package areas

example: #Area & {
	name:        "Example"
	description: "Minimal fixture; real tree lives in kolohelios/personal-os."
	children: [
		{name: "Work"},
		{
			name: "Personal"
			children: [
				{name: "Health"},
				{name: "Family"},
			]
		},
	]
}
