package games

games: tic: {
	id:   "tic"
	name: "tic"
	players: ["Jon", "Jessica"]
	model: {
		kind:   "roundPoints"
		winner: "lowest"
		scoring: {
			// 2–10 score face value; aces, face cards, and wilds are named.
			faceValue: {from: 2, to: 10}
			named: {
				ace:   20
				a:     20
				king:  10
				k:     10
				queen: 10
				q:     10
				jack:  10
				j:     10
				wild:  50
				w:     50
			}
		}
		// The player who goes out first "tics"; at game end each tic is
		// worth -5 (applied to the running total continuously, same final
		// result).
		roundAward: {label: "tic", pointsPerAward: -5}
	}
}
