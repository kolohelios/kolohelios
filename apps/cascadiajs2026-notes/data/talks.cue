package talks

// Structured metadata for every talk. The prose body for each talk
// lives at `content/talks/<slug>.md`. `build-site` exports this file
// via `cue export -e talks`, which validates each entry against
// `#Talk` as a side effect — a missing required field or wrong type
// fails the build.
//
// `slides` and `projects`/`sources` are enrichable: speaker/title are
// canonical (verify against the official schedule); slides links and
// referenced repos get filled in as they surface.

#Project: {
	name: string
	url:  string
}

#Talk: {
	slug:     string & =~"^[a-z0-9-]+$"
	speaker:  string
	company:  string
	title:    string
	day:      int & >=1
	order:    int & >=1
	slides?:  string
	projects: [...#Project] | *[]
	sources: [...string] | *[]
}

talks: [...#Talk] & [
	{
		slug:    "biilman-agent-experience"
		speaker: "Matt Biilman"
		company: "Netlify"
		title:   "AX: Agent Experience"
		day:     1
		order:   1
		projects: [
			{name: "netlify.ai", url: "https://netlify.ai"},
			{name: "openclaw.ai", url: "https://openclaw.ai"},
			{name: "axis.run", url: "https://axis.run"},
			{name: "WorkOS", url: "https://workos.com"},
		]
	},
	{
		slug:    "agent-swarms"
		speaker: "Joel Hooks"
		company: "Badass Courses"
		title:   "AI Agent Swarms Are Amazing"
		day:     1
		order:   2
		projects: [
			{name: "opencode", url: "https://github.com/sst/opencode"},
			{name: "mattpocock/skills", url: "https://github.com/mattpocock/skills"},
			{name: "agentdungeon.ai", url: "https://agentdungeon.ai"},
		]
	},
	{
		slug:    "modern-css-colors"
		speaker: "James Steinbach"
		company: "Delinea"
		title:   "Practical Refactors with Modern CSS Colors"
		day:     1
		order:   3
		slides:  "https://jdsteinbach.com/practical-color-css/"
	},
	{
		slug:    "web-workers"
		speaker: "Courtney Yatteau"
		company: "Esri"
		title:   "Keep the Main Thread Free with Web Workers"
		day:     1
		order:   4
		projects: [
			{name: "cyatteau/cascadiajs-2026-web-workers", url: "https://github.com/cyatteau/cascadiajs-2026-web-workers"},
		]
	},
	{
		slug:    "ai-to-learn"
		speaker: "Daniel Mendoza"
		company: "Storyblok"
		title:   "AI Helped Me Learn: Vue Through the Lens of a React Developer"
		day:     1
		order:   5
		slides:  "https://new.express.adobe.com/publishedV2/urn:aaid:sc:US:5b2e8fce-4bc0-5bcc-bb70-62f7c36f80b2?promoid=Y69SGM5H&mv=other"
	},
	{
		slug:    "linked-literate-programming"
		speaker: "James Ide"
		company: "Expo"
		title:   "Implementing the Web on Native with Linked Literate Programming"
		day:     1
		order:   6
		slides:  "https://github.com/expo/web-standard-camera-demo/releases/download/v1.0.0/slides.pdf"
		projects: [
			{name: "ccheever/llp", url: "https://github.com/ccheever/llp"},
			{name: "Web demo", url: "https://standard-camera-demo.expo.app/"},
			{name: "Web Platform Tests", url: "https://github.com/web-platform-tests/wpt"},
		]
	},
	{
		slug:    "wrong-abstraction"
		speaker: "Darius Cepulis"
		company: "Mux"
		title:   "Choosing the Wrong Abstraction (And What It Cost Us)"
		day:     1
		order:   7
		projects: [
			{name: "decepulis/ax-bench", url: "https://github.com/decepulis/ax-bench"},
			{name: "Observable Plot", url: "https://observablehq.com/plot/"},
			{name: "Bret Victor — Ladder of Abstraction", url: "https://worrydream.com/LadderOfAbstraction/"},
		]
	},
	{
		slug:    "shared-components"
		speaker: "Jonathan Keslin"
		company: "Atlassian"
		title:   "Shared Components Beyond the Design System"
		day:     1
		order:   8
		slides:  "https://jonathankeslin.com/cascadiajs26"
	},
	{
		slug:    "junior-dev-team"
		speaker: "Dylan Goings"
		company: "Atomic Object"
		title:   "How to Successfully Build a Junior Dev Team"
		day:     1
		order:   9
		slides:  "https://learningasleadership.my.canva.site/cascadiajs-presentation"
	},
	{
		slug:    "skeptics-to-champions"
		speaker: "Jeff Otaño"
		company: "Onebrief"
		title:   "From Skeptics to Champions"
		day:     1
		order:   10
	},
	{
		slug:    "spec-driven-development"
		speaker: "Erik Hanchett"
		company: "AWS"
		title:   "How to Use Spec-Driven Development for Production Workflows"
		day:     1
		order:   11
		projects: [
			{name: "programwitherik.com", url: "https://programwitherik.com"},
			{name: "Kiro", url: "https://kiro.dev"},
		]
	},
	{
		slug:    "last-mile-is-code"
		speaker: "Joe Duffy"
		company: "Pulumi"
		title:   "The Last Mile Is Code"
		day:     1
		order:   12
	},
]
