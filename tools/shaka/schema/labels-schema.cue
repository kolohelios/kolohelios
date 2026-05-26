package labels

#Label: {
	name:        string & !=""
	color:       =~"^[0-9a-f]{6}$"
	description: string
	// `true` if this label counts toward the scope-label policy
	// enforced by `shaka issue audit`. Type labels (e.g. `bug`,
	// `enhancement`), housekeeping labels (`duplicate`, `wontfix`),
	// and Dependabot category labels are all `false`.
	scope: bool
}

#LabelSet: {
	labels: [...#Label]
}
