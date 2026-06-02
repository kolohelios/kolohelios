Practical refactors you can do today with modern CSS color features. The
through-line: a lot of what we reached for a preprocessor to do, CSS now does
natively, **in context**.

## Theming with `light-dark()`

`light-dark()` takes two values — a light-mode color and a dark-mode color (both
must be colors) — and does the media query's job for you.

Overrides stay tiny. A six-line override (one block each for dark and light) is
enough:

```css
[data-theme="dark"] {
  color-scheme: dark;
}
```

Global support is ~87% (check caniuse for the current number); Electron already
supports it. Tailwind's `@theme` can lean on `light-dark()` too.

## Color manipulation with relative colors

Relative color syntax pulls the raw channels out of an existing color:

```css
rgb(from var(--red) r g b / 1);
```

`from` extracts the source color's values. Modern color functions drop the
commas — but alpha still needs a slash: `hsl(120 50% 20% / 0.2)`. Alpha is
implicit regardless of whether you write `rgb` or `rgba`.

This unlocks, all without a preprocessor:

- **Tints** — `rgb(from var(--surface-primary) r g 255)`. No clamping needed.
- **Adjust alpha** — `rgb(from var(--surface-primary) r g b / 0.2)`.
- **Normalized lightness** — same idea as alpha, but in `hsl`.
- **Color math** — start from a named color and rotate hue: secondary at 120°,
  tertiary at 240°.

These are the **mixins** we used SASS color functions for — now native (~89%
support). Change one line to derive a variable and operate on it right there.

## `color-mix()` and `contrast-color()`

`color-mix()` blends colors in real time, in a chosen color space:

```css
color-mix(in hsl, var(--button-color), var(--button-active-color))
```

(~91% support.)

For accessibility, `contrast-color()` returns black or white — whichever
contrasts better against a given color, regardless of color scheme. It's
**stackable**: use `color-mix()` *inside* `contrast-color()` with relative
colors to mix a tint in and still get a legible foreground. Support is only
~67% today, so progressively enhance.
