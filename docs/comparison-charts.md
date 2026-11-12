# Comparison charts

How to make a before-and-after chart for posting, such as the speed-up card for huge chats. The
chart is a hand-written SVG rendered to a PNG. Nothing is kept in the repo: the SVG and the PNG
go where the person asked for them, usually the Desktop.

## Contents of the card

- One headline number, and it is the largest thing on the card. Below it, one line that finishes
  the sentence: "faster in huge chats".
- Two or three supporting figures in a row underneath: the largest ratios for steps a user does
  and would feel, such as opening a chat. Leave out the rest, even when they were measured.
- "Patchwork" in the top left. When the headline is the best of several results, "up to" goes on
  the line directly under "Patchwork", with clear space between it and the number.
- No title, subtitle, legend, axis, gridline, data bar or footnote. If the card needs one of these to
  be understood, change the wording.

## Numbers

- Every figure comes from a measurement made in this repo, before and after the change the card
  is about (see "Performance checks" in the root `CLAUDE.md`). Divide the time before by the time
  now and round down to two significant figures:
  24 ms and 0.01 ms is 2,400×.
- Write a ratio as a number and "×", with a comma in the thousands: `2,400×`, `180×`.
- Say how it was measured when handing the card over, not on the card. A debug build is slower
  than a release, so name the build.
- When the biggest ratio is for a step people never see, tell the person, and offer the largest
  one they would feel as the headline.

## Wording

- Each supporting figure gets one lower-case line that says what the user was doing: "opening a
  41,000-item chat", "sending a message in a 75,000-item chat". Start with the "-ing" form of the
  verb.
- Use the words the app uses: "chat", not "thread". No names of functions, types or files.
- Give the size the measurement was made at, in the line itself: how many items, how many chats.
- Every ratio shows the times it came from, written "was 24 ms, now 0.01 ms". A ratio alone tells
  the reader nothing about whether the step was slow.
- The headline's times go on a grey line under "faster in huge chats", after what was measured:
  "fetching a 75,000-item chat: was 24 ms, now 0.01 ms". Each supporting figure's times go under
  its label, in the figure colour at weight 600.

## Look

| Part | Value |
|---|---|
| Canvas | 1600 × 900, rendered at 2× to 3200 × 1800 |
| Background | `#0b0b0c` |
| Font | Inter, from `crates/desktop/assets/fonts` |
| Text | `#f4f4f5` for figures and the headline line, `#b4b4bc` for labels |
| Colours | blue `#4682fa`, pink `#f25fa0`, amber `#f5a524`, green `#3ecf8e` |
| Margins | text starts at 72 and ends by 1528; the headline number sits at 62 so its first digit lines up by eye |
| Columns | supporting figures start at 72, 560 and 1048 |

- The headline number is 240 px, weight 700, filled with a gradient from blue through pink to
  amber. It is the only coloured text.
- Behind it sit three blurred circles in the same three colours, at a third of full strength, so
  the number glows without a visible shape.
- Each supporting figure is 64 px in the figure colour, with a short coloured bar above it (56 × 8,
  rounded) as decoration. The bars are pink, amber and green, left to right.
- Keep text clear of the headline's comma, which hangs about 50 px below the line the digits
  stand on.

## The file

Change the text and the numbers, and keep the positions.

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900" font-family="Inter">
  <defs>
    <linearGradient id="hero" x1="0" y1="0" x2="1" y2="0.35">
      <stop offset="0" stop-color="#4682fa"/>
      <stop offset="0.5" stop-color="#f25fa0"/>
      <stop offset="1" stop-color="#f5a524"/>
    </linearGradient>
    <filter id="blur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="110"/></filter>
  </defs>
  <rect width="1600" height="900" fill="#0b0b0c"/>
  <g filter="url(#blur)" opacity="0.33">
    <circle cx="260" cy="290" r="190" fill="#4682fa"/>
    <circle cx="600" cy="250" r="180" fill="#f25fa0"/>
    <circle cx="940" cy="320" r="170" fill="#f5a524"/>
  </g>
  <text x="72" y="104" font-size="30" font-weight="600" fill="#f4f4f5" letter-spacing="-0.4">Patchwork</text>
  <text x="72" y="156" font-size="40" fill="#b4b4bc">up to</text>
  <text x="62" y="384" font-size="240" font-weight="700" fill="url(#hero)" letter-spacing="-8">2,400×</text>
  <text x="72" y="498" font-size="66" font-weight="600" fill="#f4f4f5" letter-spacing="-1.6">faster in huge chats</text>
  <text x="72" y="546" font-size="24" fill="#b4b4bc">fetching a 75,000-item chat: was 24 ms, now 0.01 ms</text>

  <rect x="72" y="644" width="56" height="8" rx="4" fill="#f25fa0"/>
  <text x="72" y="724" font-size="64" font-weight="600" fill="#f4f4f5" letter-spacing="-1.4">1,000×</text>
  <text x="72" y="766" font-size="22" fill="#b4b4bc">drawing a frame of a 41,000-item chat</text>
  <text x="72" y="800" font-size="22" font-weight="600" fill="#f4f4f5">was 31 ms, now 0.03 ms</text>

  <rect x="560" y="644" width="56" height="8" rx="4" fill="#f5a524"/>
  <text x="560" y="724" font-size="64" font-weight="600" fill="#f4f4f5" letter-spacing="-1.4">180×</text>
  <text x="560" y="766" font-size="22" fill="#b4b4bc">opening a 41,000-item chat</text>
  <text x="560" y="800" font-size="22" font-weight="600" fill="#f4f4f5">was 18 ms, now 0.1 ms</text>

  <rect x="1048" y="644" width="56" height="8" rx="4" fill="#3ecf8e"/>
  <text x="1048" y="724" font-size="64" font-weight="600" fill="#f4f4f5" letter-spacing="-1.4">100×</text>
  <text x="1048" y="766" font-size="22" fill="#b4b4bc">sending a message in a 75,000-item chat</text>
  <text x="1048" y="800" font-size="22" font-weight="600" fill="#f4f4f5">was 48 ms, now 0.48 ms</text>

</svg>
```

With two supporting figures, put them at 72 and 560. A label has to end before the next column
starts, which is about 40 characters.

## Rendering

Rendering needs `rsvg-convert` (from librsvg). Inter is not installed system-wide, so the commands
point fontconfig at the app's fonts for the one render and leave no settings behind. Run them from
the repo root, with `f` set to the SVG's path:

```sh
f=~/Desktop/patchwork-speedups.svg
mkdir -p target/chart-scratch
printf '<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig><dir>%s/crates/desktop/assets/fonts</dir><cachedir>%s/target/chart-scratch/cache</cachedir></fontconfig>' "$PWD" "$PWD" > target/chart-scratch/fonts.conf
FONTCONFIG_FILE=$PWD/target/chart-scratch/fonts.conf rsvg-convert -z 2 "$f" -o "${f%.svg}.png"
rm -r target/chart-scratch
```

Then open the PNG and look at it before handing it over. Check that no text touches other text,
that every label ends before the next column, and that nothing runs past the right margin at 1528.
