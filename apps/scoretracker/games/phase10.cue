package games

games: phase10: {
	id:   "phase10"
	name: "Phase 10"
	players: ["Jon", "Jessica"]
	model: {
		kind:   "roundPoints"
		winner: "lowest"
		scoring: {
			ranges: [
				{from: 1, to: 9, points: 5},
				{from: 10, to: 12, points: 10},
			]
			named: {
				skip: 15
				wild: 25
			}
		}
	}
}
