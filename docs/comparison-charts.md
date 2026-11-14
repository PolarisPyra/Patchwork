# Comparison charts

How to make a before-and-after chart for posting, such as the speed-up chart for huge chats. The
chart is a hand-written SVG rendered to a PNG. Nothing is kept in the repo: the SVG and the PNG
go where the person asked for them, usually the Desktop.

## Contents of the chart

- A title that states the result as a sentence: "Patchwork got a speed boost".
- Up to six rows, one per measured step, ordered from the longest time before to the shortest.
- Each row has a label saying what the user was doing, with the size of the chat under it, and
  two horizontal bars: the time before, and under it the time now. Each bar has its time written
  at its end.
- One time axis under the rows. It starts at zero and has a faint line up from each label. A
  legend on the title's line at the right names the two bar colours: "before" and "now".
- No "times faster" figure. The two times and the two bars already say it.
- No subtitle or footnote. If the chart needs one to be understood, change the wording.

## Numbers

- Every figure comes from a measurement made in this repo, before and after the change the chart
  is about (see "Performance checks" in the root `CLAUDE.md`).
- Write each time in ms with two significant figures at most, both in the same unit: "24 ms",
  "0.34 ms". Round the way the measurement supports, never to flatter it.
- Say how it was measured when handing the chart over, not on the chart. A debug build is slower
  than a release, so name the build.
- When the biggest speed-up is for a step people never see, tell the person.

## Wording

- Each row's label is two lines. The first says what the user was doing, the way they would say
  it, starting with the "-ing" form of the verb: "Sending a message", "Opening a chat". The second
  is smaller and grey and gives the size the measurement was made at: "in a chat with 75,000
  items", "with 109 chats".
- Read the labels aloud. If one sounds like a log line ("Refresh the list of 109 chats"), say it
  the way you would to someone using the app ("Updating the chat list").
- Use the words the app uses: "chat", not "thread". No names of functions, types or files.
- Both lines have to end before the bars start, 480 across the 1600-wide canvas. That is about
  28 characters for the first line.

## Bars

- Every bar starts at zero and is drawn to scale, on one linear axis shared by all rows. A bar's
  width is its time multiplied by the axis's pixels per ms. The file below uses 20 per ms, so
  48 ms is 960 wide.
- Pick the axis so the longest bar fills most of it and the labels are round numbers: 0 to 50 ms
  in steps of 10 puts a label every 200, from 480 to 1480.
- Give the bars as much of the width as the labels leave, so short bars are as long as the scale
  allows.
- A bar that would be under 4 wide is drawn 4 wide, the least that reads as a bar, with its time
  beside it. Do not lengthen it further, draw every row's bar the same length, or switch to a
  logarithmic axis to make it visible. A "now" bar that has almost vanished is what the chart is
  there to show.
- The axis is never cut or started above zero, and no bar is drawn against a scale of its own.

## Look

| Part | Value |
|---|---|
| Canvas | 1600 × 900, rendered at 2× to 3200 × 1800 |
| Background | `#0b0b0c` |
| Font | Inter, from `assets/fonts` |
| Text | `#f4f4f5` for the title, labels and the time now; `#b4b4bc` for the time before and the legend; `#8b8b94` for the axis labels and each label's second line |
| Bars | grey `#5c5c66` for before, blue `#4682fa` for now; 20 tall, the before bar's right end rounded by 4 |
| Axis | the zero line is `#3a3a42` and the others `#1f1f23`, 1.5 thick; labels are 20 px and the last one carries the unit |
| Margins | everything sits between 56 and 1544 across |
| Rows | 104 apart; the first row's label has its 27 px line at 221 down and its 20 px line at 249 |

- Grey and blue are the only colours, and no row gets a colour of its own.
- Times are 22 px and start 12 after the end of their bar. The time now is white at weight 600
  and the time before is grey at regular weight, so the eye lands on the new number.

## The file

Every position is in canvas units, `x` across and `y` down. Change the text and the times, and
keep the positions, apart from the two that follow from a time:

- The before bar is a `path`, so only its right end is rounded. Its `h` values are the bar's
  width minus 4: 48 ms is 960 wide, so `h956` and `h-956`.
- Each time's `x` is 480 plus its bar's width plus 12.

The file has two rows. For each further row, copy a row and add 104 to every `y` in it (in the
`path`, the number after `M480`). The axis lines run from 170 down to 25 below the last row's
second label line, and the axis labels sit 30 below that. With six rows the lines end at 794 and
the labels are at 824.

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900" font-family="Inter">
  <rect width="1600" height="900" fill="#0b0b0c"/>
  <text x="56" y="112" font-size="54" font-weight="600" fill="#f4f4f5" letter-spacing="-1.2">Patchwork got a speed boost</text>
  <g font-size="22" fill="#b4b4bc">
    <rect x="1306" y="88" width="28" height="16" rx="3" fill="#5c5c66"/>
    <text x="1344" y="104">before</text>
    <rect x="1446" y="88" width="28" height="16" rx="3" fill="#4682fa"/>
    <text x="1484" y="104">now</text>
  </g>

  <line x1="480" y1="170" x2="480" y2="378" stroke="#3a3a42" stroke-width="1.5"/>
  <text x="480" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">0</text>
  <line x1="680" y1="170" x2="680" y2="378" stroke="#1f1f23" stroke-width="1.5"/>
  <text x="680" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">10</text>
  <line x1="880" y1="170" x2="880" y2="378" stroke="#1f1f23" stroke-width="1.5"/>
  <text x="880" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">20</text>
  <line x1="1080" y1="170" x2="1080" y2="378" stroke="#1f1f23" stroke-width="1.5"/>
  <text x="1080" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">30</text>
  <line x1="1280" y1="170" x2="1280" y2="378" stroke="#1f1f23" stroke-width="1.5"/>
  <text x="1280" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">40</text>
  <line x1="1480" y1="170" x2="1480" y2="378" stroke="#1f1f23" stroke-width="1.5"/>
  <text x="1480" y="408" font-size="20" fill="#8b8b94" text-anchor="middle">50 ms</text>

  <text x="56" y="221" font-size="27" fill="#f4f4f5">Sending a message</text>
  <text x="56" y="249" font-size="20" fill="#8b8b94">in a chat with 75,000 items</text>
  <path d="M480 198h956a4 4 0 0 1 4 4v12a4 4 0 0 1 -4 4h-956z" fill="#5c5c66"/>
  <text x="1452" y="216" font-size="22" fill="#b4b4bc">48 ms</text>
  <rect x="480" y="226" width="9.6" height="20" fill="#4682fa"/>
  <text x="501.6" y="244" font-size="22" font-weight="600" fill="#f4f4f5">0.48 ms</text>

  <text x="56" y="325" font-size="27" fill="#f4f4f5">Redrawing the screen</text>
  <text x="56" y="353" font-size="20" fill="#8b8b94">in a chat with 41,000 items</text>
  <path d="M480 302h616a4 4 0 0 1 4 4v12a4 4 0 0 1 -4 4h-616z" fill="#5c5c66"/>
  <text x="1112" y="320" font-size="22" fill="#b4b4bc">31 ms</text>
  <rect x="480" y="330" width="4" height="20" fill="#4682fa"/>
  <text x="496" y="348" font-size="22" font-weight="600" fill="#f4f4f5">0.03 ms</text>
</svg>
```

## Rendering

Rendering needs `rsvg-convert` (from librsvg). Inter is not installed system-wide, so the commands
point fontconfig at the app's fonts for the one render and leave no settings behind. Run them from
the repo root, with `f` set to the SVG's path:

```sh
f=~/Desktop/patchwork-speedups.svg
mkdir -p target/chart-scratch
printf '<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig><dir>%s/assets/fonts</dir><cachedir>%s/target/chart-scratch/cache</cachedir></fontconfig>' "$PWD" "$PWD" > target/chart-scratch/fonts.conf
FONTCONFIG_FILE=$PWD/target/chart-scratch/fonts.conf rsvg-convert -z 2 "$f" -o "${f%.svg}.png"
rm -r target/chart-scratch
```

Then open the PNG and look at it before handing it over. Check that no text touches other text,
that every label ends before the axis at 480, that each bar ends where its time says against the
axis labels, and that nothing runs past the right margin at 1544.
