package profile

// Canonical profile prose, shared by the rendered résumé (this project's
// resume.md) and the portfolio's about page (apps/kolohelios-portfolio).
// Only the genuinely-duplicated profile sections live here — identity,
// contact, summary, skills, education. Work-experience bullets stay
// curated per medium (the résumé is a tightened distillation of the
// portfolio's richer work history), so they are deliberately not modelled
// here. See #785.
#Profile: {
	name:  string & !=""
	title: string & !=""
	contact: #Contact
	summary: string & !=""
	// At least one skill group; rendered as résumé `## Skills` bullets and
	// the about page's "Languages & tools" list.
	skills: [#SkillGroup, ...#SkillGroup]
	education: #Education
}

#Contact: {
	phone:       string & !=""
	email:       string & !=""
	citizenship: string & !=""
	location:    string & !=""
	linkedin:    string & =~"^https://"
	github:      string & =~"^https://"
}

#SkillGroup: {
	category: string & !=""
	// The whole rendered item line (kept as one string rather than a list
	// because entries carry their own punctuation, e.g. "Scala 2; prior
	// production: Java, ...").
	items: string & !=""
}

#Education: {
	institution: string & !=""
	degree:      string & !=""
	gpa:         string & !=""
	honors:      string & !=""
}

profile: #Profile
