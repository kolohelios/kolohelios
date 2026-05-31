// Areas of focus: a strict tree of life domains. Parent-child only —
// no cross-area references, so cycles are impossible by construction.
// If cross-links ever become load-bearing, add them as a separate edge
// list and implement cycle detection in `aof`, don't pollute this shape.
package areas

#Area: {
	name:         string & !=""
	description?: string & !=""
	children?: [...#Area]
}
