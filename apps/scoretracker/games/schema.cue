// Schema for the static game definitions embedded into the Worker. Each
// `games/<id>.cue` adds one entry to the package-wide `games:` map; the
// build step exports `games` to JSON (`build.rs` runs `cue export -e
// games`) and `engine::config` deserializes it. Adding a game = a new
// `games/<id>.cue`, no engine code.
package games

#Game: {
	id:      string & =~"^[a-z0-9-]+$"
	name:    string
	players: [string, ...string] // at least one
	model:   #RoundPoints | #MatchWins
}

// Per-round point scoring: every round each player enters their remaining
// cards as free text, which maps to points; cumulative total decides the
// winner. Optionally a per-round award (e.g. "went out first") accrues an
// end-of-game adjustment.
#RoundPoints: {
	kind:        "roundPoints"
	winner:      "lowest" | "highest"
	scoring:     #Scoring
	roundAward?: #RoundAward
}

// How a single parsed token becomes points. Lookup order: a `named` token
// (case-insensitive) wins; otherwise the token is parsed as an integer and
// matched against `ranges` (first match) then `faceValue` (token value ==
// points).
#Scoring: {
	ranges?: [...#Range]
	faceValue?: #FaceValue
	named?: [string]: int
	// Whole-input aliases (matched before tokenizing, case-insensitive)
	// that mean "no cards" → zero points.
	emptyAliases: [...string] | *["no cards", ""]
}

#Range: {from: int, to: int, points: int}
#FaceValue: {from: int, to: int}
#RoundAward: {label: string, pointsPerAward: int}

// Match-win tally: no per-round scoring — each finished game records a
// single winner and the totals are win counts. `target` optionally marks a
// "first to N wins" champion (UI hint only).
#MatchWins: {
	kind: "matchWins"
	target?: int & >0
}

games: [string]: #Game
