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

		// Dealer rotates each round from a chosen round-1 dealer.
		dealer: {rotates: true}

		// Three Thirteen: 11 rounds, the wild rank climbs 3→King. A held
		// wild-rank card scores 50 (overriding its normal value — so a King
		// in the Kings-wild round is 50, not 10). The game ends after round
		// 11.
		wildProgression: {
			ranks: ["3", "4", "5", "6", "7", "8", "9", "10", "j", "q", "k"]
			points: 50
		}
	}
}
