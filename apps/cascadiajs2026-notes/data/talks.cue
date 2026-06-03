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
	slug:    string & =~"^[a-z0-9-]+$"
	speaker: string
	company: string
	title:   string
	day:     int & >=1
	order:   int & >=1
	slides?: string
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
		slides:  "https://cascadia.wzrrd.sh/"
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
	{
		slug:    "rust-critical-path"
		speaker: "Francesco Ciulla"
		company: "Zerops"
		title:   "JavaScript Won the Web. Rust Is Taking the Critical Path."
		day:     2
		order:   1
		projects: [
			{name: "Pingora", url: "https://github.com/cloudflare/pingora"},
			{name: "Tokio", url: "https://tokio.rs"},
			{name: "Axum", url: "https://github.com/tokio-rs/axum"},
			{name: "SQLx", url: "https://github.com/launchbadge/sqlx"},
		]
	},
	{
		slug:    "choosing-typescript"
		speaker: "Filip Sodić"
		company: "Wasp"
		title:   "Choosing TypeScript Matters More Than Ever"
		day:     2
		order:   2
		slides:  "https://sodic.dev/choosing-typescript-matters-cascadiajs-2026.pdf"
		projects: [
			{name: "Wasp", url: "https://wasp.sh"},
		]
	},
	{
		slug:    "on-device-ai-music"
		speaker: "Alex Hinson"
		company: "Fleetio"
		title:   "Accelerating Musical Live Coding With On-Device AI"
		day:     2
		order:   3
		slides:  "https://bit.ly/cascadia-ai-music"
		projects: [
			{name: "Strudel", url: "https://strudel.cc"},
			{name: "transformers.js", url: "https://github.com/huggingface/transformers.js"},
		]
	},
	{
		slug:    "teaching-llms-new-tricks"
		speaker: "Marty Nelson"
		company: "Works Real Estate"
		title:   "Teaching LLMs New Tricks"
		day:     2
		order:   4
		projects: [
			{name: "Azoth", url: "https://github.com/azothjs/azoth"},
		]
	},
	{
		slug:    "beowulf-stroganoff"
		speaker: "Molly Jean Bennett"
		company: "Grow Therapy"
		title:   "Beowulf Stroganoff: Building Economically Useless Chatbots"
		day:     2
		order:   5
		slides:  "https://docs.google.com/presentation/d/1mxPhGJUA2f2FTfEm0B09IbVC5MwJ-f8oEgMhEBA1A64/edit?usp=sharing"
		projects: [
			{name: "Beowulf Stroganoff (Hugging Face Space)", url: "https://huggingface.co/spaces/MollyJeanB/beowulf-stroganoff"},
		]
	},
	{
		slug:    "design-tokens"
		speaker: "Kaelig Deloumeau-Prigent"
		company: "Design Tokens W3C CG"
		title:   "Design Tokens: Getting Agents to Follow Brand Guidelines"
		day:     2
		order:   6
		projects: [
			{name: "designtokens.org", url: "https://www.designtokens.org"},
		]
	},
	{
		slug:    "hidden-connections-graphs"
		speaker: "Nyah Macklin"
		company: "Neo4j"
		title:   "Unlocking AI's Hidden Connections With Graphs"
		day:     2
		order:   7
		projects: [
			{name: "Neo4j GraphAcademy", url: "https://graphacademy.neo4j.com"},
		]
	},
	{
		slug:    "atproto-apps"
		speaker: "Brittany Ellich"
		company: "Bluesky"
		title:   "Building Apps With ATProto"
		day:     2
		order:   8
		slides:  "https://docs.google.com/presentation/d/1GCG0h4H5w-ZLJ0Gl9YkQXu1iHyivS-AhueAtUzqfqzw/edit?usp=drivesdk"
		projects: [
			{name: "ATStore", url: "https://atstore.fyi"},
			{name: "Sifa.id", url: "https://sifa.id"},
			{name: "Streamplace", url: "https://stream.place"},
			{name: "rpg.actor", url: "https://rpg.actor"},
			{name: "atmo.quest", url: "https://atmo.quest"},
		]
	},
	{
		slug:    "request-tax"
		speaker: "Alex Moon"
		company: "WP Engine"
		title:   "The Request Tax: Re-evaluating 20+ Years of Web Performance Dogma"
		day:     2
		order:   9
		slides:  "https://github.com/moonmeister/request-tax/tree/cascadia-js"
	},
	{
		slug:    "human-in-the-loop"
		speaker: "Michael Liendo"
		company: "Auth0"
		title:   "Trust, But Verify: Human-in-the-Loop for Agents That Actually Matter"
		day:     2
		order:   10
		projects: [
			{name: "Auth0", url: "https://auth0.com"},
		]
	},
	{
		slug:    "karaoke-stems"
		speaker: "Luis Montes"
		company: "Iced Dev"
		title:   "Hold Me Closer, Tony Danza"
		day:     2
		order:   11
		projects: [
			{name: "Demucs", url: "https://github.com/adefossez/demucs"},
			{name: "Whisper", url: "https://github.com/openai/whisper"},
			{name: "Butterchurn", url: "https://github.com/jberg/butterchurn"},
			{name: "CREPE", url: "https://github.com/marl/crepe"},
		]
	},
	{
		slug:    "rethink-everything"
		speaker: "Theo"
		company: "T3 Chat"
		title:   "It's Time to Rethink Everything"
		day:     2
		order:   12
		projects: [
			{name: "Lakebed", url: "https://lakebed.dev"},
			{name: "shoo.dev", url: "https://shoo.dev"},
		]
	},
]
