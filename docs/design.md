# Design

How eave looks and moves, and why. The values live in [`crates/eave/src/theme.rs`](../crates/eave/src/theme.rs); this page says what they're for. The choices were made by comparing variants side by side ([D-61](decisions.md)). The bar comes in two schemes, dark and light, each the other's mirror ([D-92](decisions.md)): this page describes the dark one, and the light one swaps black and white.

## The feel

**Minimal at rest, alive when touched.** At rest the bar is a few black shapes with white text in them. When something changes, or a hand reaches for it, it moves: shapes stretch, marks slide, numbers count. The moment it's left alone, it goes quiet again, and draws no frames.

## Five rules

1. **Black islands, white ink** (or, light, white islands and black ink). Islands and popovers are black. What's in them is white. Hierarchy comes from brightness: full white for the thing to notice, 94 % for text, 50 % for secondary text. There are no tinted greys.
2. **Color is a signal.** Color shows up only when it tells you something:
   - **state:** charging (good), low (warning), critical, a failed connection;
   - **content:** an app's icon, a track's cover, the color taken from that cover, and code, colored by what each piece of it is ([D-65](decisions.md)).

   Color never means "this is selected" or "you can press this". When everything is black and white, the one green bolt or red number gets noticed.
3. **What's current is brighter.** The workspace you're on, the row in use, the open tab and today in the calendar are all marked the same way: with light, as a soft white fill, a white ring or a heavier weight. Never with a hue.
4. **Everything morphs.** Nothing pops in or out. A popover grows out of its island and back into it, an island springs to its new width, a mark slides to the new option, a number counts through the values in between, and a page slides as it hands over to the next. Each of these motions has one spring or one duration in `theme.rs`, shared by everything that does it.
5. **It responds to the hand.** What's under the pointer reacts: an island squeezes when pressed, with either button, and stretches when dragged, a slider's line thickens, a button lights up. At rest everything is thin and quiet.

## Palette

| Token | Value | Used for |
|---|---|---|
| island | black | islands, popovers, notification cards |
| `Tone::Accent` | white | the thing to notice; text on what's current |
| `Tone::Normal` | white at 94 % | text and icons |
| `Tone::Dim` | white at 50 % | secondary text, workspaces you aren't on, the off toggle's knob |
| `TRACK` | white at 14 % | the empty part of sliders, toggles and meters |
| `SWITCHER_MARK`, `TAB_MARK` | white at 14 %, 16 % | the current workspace, the current tab |
| `SELECTED_BG` | white at 10 % | the list row in use |
| `HOVER_BG`, `PRESSED_BG` | white at 8 %, 14 % | under the pointer |
| `ACCENT_FILL` | white | the on toggle, a prominent button |
| `Tone::Good` | `#30D158` | charging, healthy |
| `Tone::Warning` | `#FFB020` | low, attention |
| `Tone::Critical` | `#FF453A` | critical, failed, a critical notification |
| content | from the cover | a media popover's slider and play button |
| `CODE_KEYWORD`, `CODE_TYPE`, `CODE_FUNCTION`, `CODE_STRING`, `CODE_NUMBER` | soft lavender, cyan, sand, sage, apricot, all about as light | code: keywords, types, calls, strings, numbers |
| `CODE_PLAIN`, `CODE_COMMENT`, `CODE_PUNCTUATION` | near white, slate grey (italic), grey | the rest of the code, and what sits back |
| `DIFF_ADDED_BG`, `DIFF_REMOVED_BG` | `Tone::Good`, `Tone::Critical` at 12 % | behind a diff's added and removed lines |

`Tone::Normal` and `Tone::Dim` are written as the grey they make on black, not as translucent white, so the strokes of an icon that overlap don't show seams. Islands are always black, so it's the same color.

**Light.** `theme = "light"`, or `"system"` on a desktop that prefers light ([D-92](decisions.md)): islands are white, `Tone::Normal` black at 88 % and `Tone::Dim` at 50 % (as greys on white), `Tone::Accent` and the accent fill black, with white on them. Every white overlay above is black at about the same alpha. Signals and code are deeper, to read on white: good `#248A3D`, warning `#C25E00`, critical `#D70015`. The notch and the grown top edge stay black in both: they stand for the screen's black border. The dormer drops out of them black, and turns white as it unfolds; it goes black again before it rises back in.

**Glass.** With the hypr-eave plugin, an island's fill turns to glass: what's behind it, frosted and tinted by its own color ([D-112](decisions.md)). Grey ink over it takes the opposite of the glass's color, as light or dark as it takes to read. Colored content, like sprites and signals, keeps its own colors. Glass is a theme of its own, drawn with the dark palette: over something bright, the island's ink turns dark instead, by the plugin, and springs there rather than snapping. Popovers, the dormer and dialogs do the same. A drop is black while it hangs from the notch, and glass once it lets go. These colors come from what's behind, like content color does, never from a new token. The notch, the grown edge, attached islands and a solid bar stay black.

**Etched** ([D-126](decisions.md)). On glass, a control is shaped in the glass instead of painted on it. A button, a toggle, a groove or a tab track is a well cut `RELIEF_WELL` into it; what's current (the row in use, the tab, the switcher's mark) is a bead standing `RELIEF_BEAD` on it. A list row is level until pressed. Each has its own small bevel, so the glass bends at its walls and a line of light runs round them. A press deepens the well, most under the finger. What's on (a toggle, the main button, a slider's fill) turns the glass milky, with its ink dark on it. A light sits under the pointer, faint while over, bright while pressed (`HOVER_LIGHT`). The fills for hover, press and at rest give way to all this: they'd be grey ink on glass.

**Content color.** When a popover shows a large cover, the host finds the cover's strongest hue: the hue with the most saturated, bright pixels. It then lifts that hue to a lightness that reads on black (or deepens it, on white), and uses it for that popover's slider fill and main button. A cover with little color in it (black and white, grey) gives none, and they stay white. Plugins never pick this color: the host takes it from the content ([D-11](decisions.md)).

## Components

| Component | At rest | Current or on | Under the pointer |
|---|---|---|---|
| Slider | in the bar, a 2 px line, white up to the value, 14 % after; in a popover, a 28 px groove, 14 % with white up to the value ([D-127](decisions.md)) | | the line eases to 4 px, the groove brightens; a click eases the fill there, a drag follows exactly, and past an end stretches it, less and less, springing back when let go |
| List row | plain | soft white fill, semibold title, white icons | 8 % fill |
| Switcher (workspaces) | dim text | a 14 % white pill slides behind it, its leading edge first | |
| Tabs | dim labels on an 8 % track | a 16 % pill slides behind, its leading edge first; the label is white and semibold | |
| Toggle | 14 % pill, dim knob | white pill, black knob; the knob slides over | |
| Prominent button | white disc, black icon (or the content color) | | a little dimmer |
| Text button | 8 % fill | | 14 % fill |
| Corners | a popover's are its island's and `PANEL_PADDING`; what's in it nests, each corner its container's less the inset, never under `MIN_ISLAND_RADIUS`: 26, then 14 for a control, then 11 for a mark in it ([D-125](decisions.md)) | | |
| Under the hand | | | a popover's controls squeeze a few pixels toward the finger and spring back past rest ([D-127](decisions.md)) |
| Mark (today) | a 1.5 px white ring, semibold | | |
| Count badge (`counted`) | a `Tone::Critical` circle (a pill from two digits) with a white semibold number, on the top right corner of what it counts, overlapping it a little ([D-82](decisions.md)) | | |
| Meter | a 6 px bar in its tone (white unless it's a signal) | eases to a new value | |
| Text field | 6 % fill, 10 % edge | 50 % white edge when focused; a signal color on error | |
| Timeline | a 1 px line at 14 % white, a mark per step: you a white dot, a note a small grey one, done grey | under way: a white ring with a faint halo, still; failed: a red dot; waiting for you: an amber ring | |
| Island attached to the top | hangs from the edge, its corners there out of sight; with a measured notch, rounded outwards where it meets the edge, like the notch ([D-72](decisions.md)) | | |
| Island corners | follow what they face: a screen corner at an even distance, the notch's corners beside it, `islands` everywhere else ([D-73](decisions.md)) | | |
| Island height | 30 px; with a notch, ending where the notch does ([D-76](decisions.md)) | | |
| Notch | drawn black by the bar, `grow` px bigger than it is if asked ([D-75](decisions.md)); pinches in as the dormer leaves it and flattens as it lands, within its `grow` ([D-86](decisions.md)); stays when the bar hides ([D-106](decisions.md)) | a click brings the dormer out | squeezes a little when pressed, stretches toward a drag, never smaller than it is |
| Top edge | drawn black by the bar, `grow` px thicker than it is if asked; what hangs from the top hangs from there ([D-81](decisions.md)); without a notch, bumps down where the dormer leaves and dents up where it lands ([D-86](decisions.md)); stays when the bar hides | without a notch, a click in its middle brings the dormer out | pulled down under a drag, in a long curve that follows the pointer |
| Solid bar | the top edge down to the islands' bottom, black, its lower ends rounded like the screen's top corners or square; the islands are black in it, and what grows out of one (a popover, the notifications) flows out of the bar's bottom, the shoulders growing with it from nothing; the dormer drops out of its middle ([D-89](decisions.md)) | | |
| Notification card | where it's from in small dim type, the time at its end; what it says; buttons sharing its width, the first filled; a wide picture full width under the text | a critical one's source in red with a dot; a stack's earlier notices a dim line each | the time turns into the close button ([D-85](decisions.md)) |
| Keycaps (`keys`, a row's shortcut) | each key on its own small cap: an 8 % fill, 4 px corners, its name in small semibold type in the text's tone, modifiers first (`Ctrl`, `Alt`, `Shift`, `Super`); at a row's end, dim ([D-91](decisions.md)) | on the row in use, white like its trailing | |
| Option (on a notification) | a row: its label, what it means dim under it, a "Recommended" badge | picked: lit like the row in use; with several to pick, a white check box | 8 % fill |
| Code block, diff | 7 % fill, monospace, colored for the language; four lines or more are numbered | a diff's added and removed lines on faint green and red, its header counting them | |

## Motion

All motion is driven by the compositor's frame callbacks, and stops asking for frames when it settles.

| Motion | Where | How |
|---|---|---|
| Morph | an island's shape and items, a popover opening, the dormer dropping into the middle, a dialog coming out of an island's shape, popover or not, and back | the frame rides `MORPH`, close to critical, and settles without wobbling. What's drawn is the body ([D-118](decisions.md)): a ring of points, each springing to its place on the new outline (`BODY_RESPONSE`, `BODY_DAMPING`), so it goes past and bounces back. Where it grows, the middle of the side runs on ahead (`BODY_LEAD`, `BODY_FOCUS`); where it shrinks, it hangs on and lands (`BODY_LAG`, `BODY_LAND`). Tension smooths it (`BODY_TENSION`), it keeps its area (`BODY_AREA`), and nothing goes up past the top |
| Content | a popover's or dialog's content coming in as the island turns into it, and going as it turns back | an island turning into its own popover crosses pictures of the two sides, riding the shape, the island's blurring out as the panel's sharpens in from `CONTENT_BLUR`, by how far the shape has come (`CROSS_*`, [D-124](decisions.md)). Otherwise the island's items make way on `MAKE_WAY`, then the content fades on `MORPH`, dropping `CONTENT_DROP` into place; going, it fades and lifts back toward the island while the shape shrinks, before the items come back. A dialog and the island it comes out of are one shape while they're close: a neck pulls out between them within `JOIN_BLEND` and pinches off as it leaves, and it flows back in as it lands, under the island's icons ([D-122](decisions.md)) |
| Drop | the dormer coming out of the notch or the edge, and a widget's dialog asked for out of the blue ([D-90](decisions.md)), and back. Without a notch but with an island in the center, they come out of that island instead, as a dialog comes out of what opened it ([D-106](decisions.md)) | `MORPH` through its stops: a ball swelling where it comes out, the ball fallen out on a neck that pinches off, then the panel (`DROP_*`, `NECK_*`); it bounces on `REACT` like a nudge as the neck lets go, as it lands, and as the neck catches it back (`DROP_BOUNCE`); the notch or edge squishes on `REACT` (`NOTCH_*`, `EDGE_*`) |
| Lift | the bar sliding off the top and back, when it shows on hover or hides over a fullscreen window | `MORPH`: drawn up by the surface's height. It stays out `HIDE_AFTER` after the pointer leaves, and a peek keeps it out `REVEAL_SHOWN`. A hidden bar is brought out by a `REVEAL_EDGE` strip at the top ([D-88](decisions.md)). The notch and the grown edge stay; a solid bar's band goes, and over a fullscreen window they go with the bar ([D-106](decisions.md)) |
| Press, drag | an island, the notch or the grown edge under the hand | `HAND`: overshoots a little when let go. An island never gets nearer the screen's sides and top than `WALL_GAP`, round its corners too ([D-104](decisions.md)) |
| Skin | every island's outline at rest: a ring of springs on `REACT` over the shape's frame ([D-111](decisions.md)) | what pokes it (nudges, shelves, walls, a drop's bounce) spreads round it, and the ring keeps its area (`SKIN_AREA`), so a side pushed in bulges the others. The top hangs from the bar. A morph's own motion is the body's |
| Nudge, push, jelly | a volume change, a workspace switch, a pushed neighbor | `REACT`: `NUDGE_*`, `PUSH_*`, `JELLY_*`, pokes into the skin, which smooths them out. Up or down, a flat shelf over the item, its ends easing into the edge, held out while nudges keep coming and let go `NUDGE_HOLD` after the last ([D-105](decisions.md)). A push into the screen's edge bounces off it, `WALL_GAP` short, and the side that hit squashes (`WALL_*`, [D-104](decisions.md)) |
| Mark slide | switcher, tabs | each edge on `MORPH`, the leading one in `BODY_LEAD` of its time and the trailing one in `BODY_LAG`: it stretches across and draws in after itself ([D-127](decisions.md)) |
| Press | a popover's buttons, rows, options, toggles | `HAND`: in by `CONTROL_PRESS_INSET`, scaled about a point across from the finger, so the side under it gives most; over a little when let go ([D-127](decisions.md)) |
| Stretch | a slider dragged past an end | `hand::rubber_band` toward `SLIDER_STRETCH`, back on `HAND` |
| Hand-over | tabs' pages, alternates, a popover's pages (a list to a row's details), a calendar's months (`page`) | `TAB_FADE` / `ALTERNATE_FADE`, sliding out the old way and in the new; a `page` with a higher order comes in from the right ([D-107](decisions.md)) |
| Count | counters, a count badge's number | `COUNTER_COUNT`, through every value in between |
| Grow | a count badge coming and going, and its width | `MORPH_*`, about its own middle |
| Cascade | items that come or go together: an island's collapsible items shown or hidden | the island's edge springs on `MORPH` at once; each item comes out once the chevron has passed it and goes back once it reaches it, fading and sliding `CASCADE_SLIDE` on `FADE` from or toward the chevron ([D-116](decisions.md)) |
| Ease | toggle knob, meter fill, slider line | `TOGGLE_SLIDE`, `METER_EASE`, `SLIDER_GROW`, `SLIDER_EASE` |
| Gulp | charging battery, something filling | `BATTERY_GULP`, `GULP`, at 30 fps |
| Glide | a scrolling list, `scroll`, a code block | a touchpad fling slows over `SCROLL_GLIDE_DECAY` and stops below `SCROLL_GLIDE_MIN`; a wheel step eases over `SCROLL_WHEEL_EASE` |
| Flipbook | a plugin's sprite (eave-claude's mascot) | `SPRITE_FRAME` a frame, `SPRITE_PLAYS` loops when it changes, then rests; a frame only when the picture changes |

Springs are for shapes and things the hand moves; they may overshoot a little. Eases are for values and marks; they never overshoot.

The springs speak one motion language ([D-83](decisions.md)), so everything that moves feels like the same stuff. There are three feels, each a response (how long one swing takes) and a damping ratio, in `theme.rs`. Anything the hand moves rides `HAND`. Anything an event kicks out and back rides `REACT`. Anything going somewhere rides `MORPH`. A motion says how far it goes, in pixels, and the feel works out the kick. `REACT_SIZE` scales every reaction at once. An outline keeps its area through the skin (`SKIN_AREA`); `BULGE` is how much a squish along one axis pulls the other in for everything else that squishes (an item's kick, the notch).

## Adding something new

- Start from black and white. Show a state with brightness first. Use a signal color only when the state is news (failing, low, charging). Use the content color only when the content has one.
- Mark "current" the way the table above does. Don't invent a new way.
- Animate any change of shape or value, and reuse an existing spring or duration from `theme.rs`. A new reaction rides `HAND`, `REACT` or `MORPH`, says how far it goes rather than how fast, and never gets a feel of its own. Add a new constant only if none of them fits, and say why next to it.
- Keep it thin at rest. Let it grow under the pointer, not before.
- Plugins never see any of this. They name a `Tone` or a primitive, and the host draws it ([D-11](decisions.md), [D-13](decisions.md)).
