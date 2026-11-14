# ADR 0021: The daemon hears first and second crack and marks them; the person's mark wins

## Status
Accepted (2026-11-14). Built in steps, one pull request each (see
"Building it"). Decided with the roaster on a design canvas; the one open
point (does the recipe follow a heard mark) was taken as proposed.

## Context
Marking first crack by ear is the moment a home roaster is most likely to
miss: it comes while the burner and the air want attention, and many recipe
steps and the development time ratio hang on it (ADR 0019). The roadmap's M6
asks for first crack heard within ±10 s of the hand-marked time in most of a
set of recorded roasts.

What is known about the sounds:

- **First crack** is louder, lower and slower: single popcorn-like pops,
  most energy around 800 Hz, seconds apart at first.
- **Second crack** is about 15 % quieter, with a flat spectrum up to about
  15 kHz and roughly five times the rate of events (P. S. Wilson, "Coffee
  roasting acoustics", JASA Express Letters 135 (2014); Euronoise 2018,
  paper 153, on a small roaster: second crack above 6 kHz).
- The fan is the loudest source in a Cubean+; the drum and the burner add a
  low hum.
- The roaster's 25 real roasts on the Cubean+ reached first crack at
  175–203 °C bean temperature, 192 °C on average.

The only public data is `syamaner/coffee-first-crack-audio` on Hugging Face
(Apache-2.0, dataset and model): 1,435 ten-second clips at 16 kHz, about four
hours from 21 roasts with four microphones, labelled "first crack / no first
crack", no second crack. Its model is an 87M-parameter audio transformer.
Under ADR 0007 it may be used; it is too large to run in the daemon, and
16 kHz cannot carry second crack.

The microphone is the MacBook's built-in one; the MacBook stands near the
roaster.

## Decision

### 1. In the daemon, at 48 kHz, two bands
`roast-audio` captures the microphone with `cpal` in the daemon, so it hears
while the app's window is closed and MCP clients see what it hears. It takes
the device's native rate (48 kHz on a Mac) and does not resample; below
32 kHz the high band is missing and second crack is not listened for. The
older sketch of 16 kHz is dropped.

Every 10 ms (a 1024-point FFT at 48 kHz, Hann window, hop 480) it computes
the spectral flux, the rise in log magnitude, in two bands:

- **crack band** 0.4–4 kHz, for first crack;
- **high band** 6–16 kHz, for second crack.

The hum of drum and burner lies below the crack band and is left out.

### 2. Finding pops
A pop is a peak of a band's flux far above the noise of the last 3 s: above
the median plus *k* times the median absolute deviation and at least a few
dB (low: 8 × MAD and 4 dB, normal: 6 × MAD and 3 dB, high: 4.5 × MAD and
2.5 dB), at least 50 ms after the previous pop of that band, and the band
must have lost half of its rise 30 ms later: a crack is over at once, a
voice or a clatter is not. A pop in the high band that is also strong in the crack
band counts as a crack-band pop. Each pop carries its time, band and
strength in dB above the band's noise.

### 3. From pops to cracks: rate and common sense
A single pop says little; the rate says a lot.

- **First crack start**: at least 5 crack-band pops within 20 s (4 at high
  sensitivity, 6 at low). It is placed at the **first pop of that series**,
  not at the moment the rule fires, reaching back to single pops just
  before it (at most 15 s apart, at most 30 s back): a first crack often
  starts with a few lone pops. It is only believed after CHARGE, after
  dry end when dry end is marked, and while the bean temperature is inside
  the window of §4.
- **First crack end**: after first crack, the last pop before 30 s without
  a crack-band pop; marked once the 30 s have passed.
- **Second crack start**: at least 8 high-band pops within 10 s, after first
  crack, with the bean temperature at least 12 °C above first crack's.
- Nothing is heard outside a recording.

### 4. The temperature window
First crack is believed between **170 and 205 °C**. Once a green coffee lot
has three roasts with a marked first crack, the window narrows to the lot's
mean ± 12 °C, never outside 170–205. The same numbers give the prediction
(§7). They live in `roast-core::crack`.

### 5. The heard crack is marked; the person's mark wins
A heard crack is set as an event with origin `detector`, with a gong and a
banner in the app: "First crack heard and marked: 7:41 · 195.1 °C", with
**Undo** and **Mark now**.

- A person's mark of the same kind **replaces** the heard one, as a mark
  replaces a detected dry end today; the alarm `event_replaced` says so.
- **Undo** removes the heard mark, and the detector does not mark that kind
  again in this roast. The person marks it by hand.
- A heard mark never replaces a person's or an AI's mark.
- Mode **show** only shows the pops and the rate and marks nothing; **off**
  does not open the microphone.

### 6. The recipe follows a heard first crack
Recipe steps anchored to first crack ("FC +0:30: burner 35 %") run from the
heard mark, as from a marked one. The temperature window guards against a
false alarm long before. When the person moves the mark (marks it anew),
steps not yet sent move with it; steps already sent stay. The detector never
touches burner, air, drum, filter or cooling itself: it only sets marks, and
everything that follows from a mark goes the usual way through the safety
layer.

### 7. Prediction without a microphone
While recording, the daemon expects first crack at the bean temperature
where it came in earlier roasts: of the same lot, else of all roasts on the
machine (simulator roasts only in the simulator), else at 192 °C. The
reference's first crack wins when it has one. With the current rate of rise
this gives "First crack in ~0:50 at ~196 °C". The snapshot's `first_crack`
carries the temperature, the window and what it is based on.

### 8. What is shown
- A microphone pill above the curve: listening, the pops of the last 20 s,
  or why it does not listen (off, no permission, no device).
- A strip of pops under the curve, with the series that made the mark.
- A **sound view** to watch along: the live spectrogram of the last 60 s on
  a logarithmic axis 100 Hz–16 kHz, the two bands shaded, pops as bright
  streaks, the rate against its threshold. Detection runs the same whether
  the curve or the sound view is shown.
- Settings: device, mode (mark / show / off), sensitivity, a calibration
  per air level, and recording the sound.
- The archive shows the pops of a saved roast, "heard 7:41 / marked 7:43".

The daemon sends the app a WebSocket message `audio` ten times a second: the
new spectrogram columns (64 log-spaced bands, one byte each, base64), the
new pops, the two band levels and the rates. The snapshot carries a compact
`audio` status for the app and for MCP.

### 9. Privacy
Sound stays in memory, only during a recording, and is gone after analysis.
The roast keeps the pop times and strengths and one level per second.
Recording the sound is **off by default**; when on, it is written as FLAC
next to the roast (about 35 MB per roast), stays on the computer and is never
sent. macOS asks once for the microphone for the app; the daemon runs as the
app's child, so the permission is the app's.

### 10. Measured, then a model
- Recordings with the person's marks are the test set. A command plays them
  through the detector and shows the deviation from the marks. Target from
  the roadmap: ±10 s in most roasts.
- **Calibration**: during the preheat (it listens then, without marking)
  and between pops in a roast the daemon learns each band's noise at each
  air level and shows how far the pops rise above it ("+14 dB above the
  fan"). The pop threshold already follows the noise of the last seconds,
  so a louder fan needs no fixed offset; the learned levels show whether
  the microphone stands well.
- **Crack mixer**: real crack pops cut from the person's recordings and the
  public dataset, mixed into the machine's own fan noise at each air level,
  with a crescendo and decay like a real first crack and distractors (spoon,
  voices, the cooling tray). Exact labels for training and testing, and the
  simulator's test cracks. Popcorn only from CC0/CC-BY sources and only for
  variety; nothing from YouTube.
- **A small model** trained on those recordings and mixed sound. It replaces
  the rules only where it hits the person's marks better. Built (step 6) as
  a network with one hidden layer of 16 units in plain Rust, not the CNN in
  ONNX run with `tract` first sketched: on one second of the spectrogram
  (pooled to 16 bands: mean, peak and flux) and the pop counts of the last
  1–20 s, a window every half second, it gives the chance that first crack is
  going on; seven of the last ten windows decide. It has about 1,150 weights,
  trains in seconds on the person's machine (`roastmcp audio train`) and needs
  neither a runtime nor a file from elsewhere. Training measures it against
  the rules on marked roasts it was not trained on (up to five folds), and
  the daemon lets it decide first crack start only when it hit more marks
  within ±10 s. It decides under the same common sense as the rule (after
  CHARGE and dry end, inside the temperature window, once) and needs at
  least two crack-band pops in the last 20 s; first crack is placed at the
  first pop of that series. First crack end and second crack stay with the
  rules. On synthetic mixes alone it is no better than the rules, so a
  model trained without marked roasts is kept but does not decide. The
  public dataset is not used yet: its 16 kHz clips lack the 48 kHz
  spectrogram the features come from.

### 11. The simulator cracks too
Connected to the simulator, the daemon listens to a synthetic source instead
of the microphone: fan noise with pops at the simulated first and second
crack, sped up with the simulator. The whole chain from pops to marks to
recipe steps can be tried without a roaster.

## Building it
1. Prediction (§7) and this ADR.
2. `roast-audio`: FFT, bands, pop finder, crack logic, spectrogram columns,
   a synthetic source; `roastmcp audio analyze` for recorded files.
3. The daemon: capture, settings, marks, alarms, WebSocket, API and MCP, the
   simulator's cracks.
4. The app: pill, pop strip, banner, sound view, settings, archive.
5. Calibration, recording the sound, evaluation against the marks, crack
   mixer.
6. The small model (`roastmcp audio train`).

## Consequences
- The app needs the microphone permission (`NSMicrophoneUsageDescription`,
  and the `audio-input` entitlement under the hardened runtime).
- A false first crack inside the window would start first-crack recipe steps
  early. The window, the rate rule and Undo limit this; the evaluation on
  recorded roasts shows how often it happens before the rules are trusted
  more.
- `EventOrigin::Detector` marks are replaced by every other origin, like
  `Auto`.
