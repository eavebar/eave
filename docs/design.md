# Design

How eave looks and moves, and why. The values live in [`crates/eave/src/theme.rs`](../crates/eave/src/theme.rs); this page says what they're for. The choices were made by comparing variants side by side ([D-61](decisions.md)).

## The feel

**Minimal at rest, alive when touched.** At rest the bar is a few black shapes with white text in them. When something changes, or a hand reaches for it, it moves: shapes stretch, marks slide, numbers count. The moment it's left alone, it goes quiet again, and draws no frames.

## Five rules

1. **Black islands, white ink.** Islands and popovers are black. What's in them is white. Hierarchy comes from brightness: full white for the thing to notice, 94 % for text, 50 % for secondary text. There are no tinted greys.
2. **Color is a signal.** Color shows up only when it tells you something:
   - **state:** charging (good), low (warning), critical, a failed connection;
   - **content:** an app's icon, a track's cover, the color taken from that cover, and code, colored by what each piece of it is ([D-65](decisions.md)).

   Color never means "this is selected" or "you can press this". When everything is black and white, the one green bolt or red number gets noticed.
3. **What's current is brighter.** The workspace you're on, the row in use, the open tab and today in the calendar are all marked the same way: with light, as a soft white fill, a white ring or a heavier weight. Never with a hue.
4. **Everything morphs.** Nothing pops in or out. A popover grows out of its island and back into it, an island springs to its new width, a mark slides to the new option, a number counts through the values in between, and a page slides as it hands over to the next. Each of these motions has one spring or one duration in `theme.rs`, shared by everything that does it.
5. **It responds to the hand.** What's under the pointer reacts: an island squeezes when pressed and stretches when dragged, a slider's line thickens, a button lights up. At rest everything is thin and quiet.

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

**Content color.** When a popover shows a large cover, the host finds the cover's strongest hue: the hue with the most saturated, bright pixels. It then lifts that hue to a lightness that reads on black, and uses it for that popover's slider fill and main button. A cover with little color in it (black and white, grey) gives none, and they stay white. Plugins never pick this color: the host takes it from the content ([D-11](decisions.md)).

## Components

| Component | At rest | Current or on | Under the pointer |
|---|---|---|---|
| Slider | a 2 px line, white up to the value, 14 % after | | eases to 4 px; a click eases the fill there, a drag follows exactly |
| List row | plain | soft white fill, semibold title, white icons | 8 % fill |
| Switcher (workspaces) | dim text | a 14 % white pill slides behind it | |
| Tabs | dim labels on an 8 % track | a 16 % pill slides behind; the label is white and semibold | |
| Toggle | 14 % pill, dim knob | white pill, black knob; the knob slides over | |
| Prominent button | white disc, black icon (or the content color) | | a little dimmer |
| Text button | 8 % fill | | 14 % fill |
| Mark (today) | a 1.5 px white ring, semibold | | |
| Meter | a 6 px bar in its tone (white unless it's a signal) | eases to a new value | |
| Text field | 6 % fill, 10 % edge | 50 % white edge when focused; a signal color on error | |
| Timeline | a 1 px line at 14 % white, a mark per step: you a white dot, a note a small grey one, done grey | under way: a white ring with a faint halo, still; failed: a red dot; waiting for you: an amber ring | |
| Island attached to the top | hangs from the edge, its corners there out of sight; with a measured notch, rounded outwards where it meets the edge, like the notch ([D-72](decisions.md)) | | |
| Island corners | follow what they face: a screen corner at an even distance, the notch's corners beside it, `islands` everywhere else ([D-73](decisions.md)) | | |
| Code block, diff | 7 % fill, monospace, colored for the language; four lines or more are numbered | a diff's added and removed lines on faint green and red, its header counting them | |

## Motion

All motion is driven by the compositor's frame callbacks, and stops asking for frames when it settles.

| Motion | Where | How |
|---|---|---|
| Morph | an island's shape and items, a popover opening | `MORPH_*` spring, close to critical: settles without wobbling |
| Press, drag | an island under the hand | `SPRING_*`, `DRAG_*`: overshoots a little when let go |
| Squish, splat | a popover setting off, landing back | `SQUISH_*` on top of the morph |
| Nudge, push, jelly | a volume change, a workspace switch, a pushed neighbor | `NUDGE_*`, `PUSH_*`, `JELLY_*`: the outline is jelly, poked and settling |
| Mark slide | switcher, tabs | `SWITCHER_SLIDE`, eased |
| Hand-over | tabs' pages, alternates | `TAB_FADE` / `ALTERNATE_FADE`, sliding out the old way and in the new |
| Count | counters | `COUNTER_COUNT`, through every value in between |
| Ease | toggle knob, meter fill, slider line | `TOGGLE_SLIDE`, `METER_EASE`, `SLIDER_GROW`, `SLIDER_EASE` |
| Gulp | charging battery, something filling | `BATTERY_GULP`, `GULP`, at 30 fps |
| Flipbook | a plugin's sprite (eave-claude's mascot) | `SPRITE_FRAME` a frame, `SPRITE_PLAYS` loops when it changes, then rests; a frame only when the picture changes |

Springs are for shapes and things the hand moves; they may overshoot a little. Eases are for values and marks; they never overshoot.

## Adding something new

- Start from black and white. Show a state with brightness first. Use a signal color only when the state is news (failing, low, charging). Use the content color only when the content has one.
- Mark "current" the way the table above does. Don't invent a new way.
- Animate any change of shape or value, and reuse an existing spring or duration from `theme.rs`. Add a new constant only if none of them fits, and say why next to it.
- Keep it thin at rest. Let it grow under the pointer, not before.
- Plugins never see any of this. They name a `Tone` or a primitive, and the host draws it ([D-11](decisions.md), [D-13](decisions.md)).
