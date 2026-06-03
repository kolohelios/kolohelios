## Theming

Practical refactors with modern CSS colors

`jds.li/css-colors`

CSS abstraction — how do we DRY our CSS?

Use a pre-processor?

`light-dark()` — two values: light mode color, dark mode color. Must be a color.

Already doing the media-query's job.

What about overrides?

6-line override (one each for dark and light):

```css
[data-theme="dark"] {
  color-scheme: dark;
}
```

**87% global support** — get more accurate on `caniuse`.

`Electron` already has support.

`tailwindcss` — `@theme` — `light-dark`?

## Color Manipulation

CSS relative colors:

```css
rgb(from var(--red), r, g, b, alpha);
```

`from` pulls out the raw color value from the variable.

Don't need commas anymore in color functions — but alpha needs a slash:

```css
hsl(120 50 20 / 0.2)
```

Alpha is an implicit value — regardless of whether you use `rgba` or just `rgb`.

## Tints

```css
from var(--surface-primary) r g 255
```

Do we need to clamp this? No, we don't.

## Adjust Alpha

```css
from var(--surface-primary) r g b / 0.2
```

## Normalized Lightness

Like alpha, but with `hsl`.

## Color Math

Start from a named color.

- rotate secondary 120°, 67 33
- rotate tertiary 240°, 67 33

## Mixins

`SASS` color functions — requires pre-processor.

Now we can do this in CSS — set the var and do stuff with it.

Change one line to get the var — happens in context.

**89% support globally.**

CSS `color-mix()` — new function to manipulate colors in real time. Color space, use `hsl`.

```css
color-mix(in hsl, var(--button-color), var(--button-active-color))
```

**91%**

## Automatic Contrast

Accessibility.

`contrast-color()` — returns white or black, regardless of color scheme, whichever is better for contrast.

Use `color-mix()` inside `contrast-color()` — `from` — with relative colors.

Mix the tint in.

**All stackable.**

Only **67%** right now.
