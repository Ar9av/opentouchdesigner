---
name: otd-python
description: Write Python in OpenTouchDesigner — parameter expressions, Script DATs, and the Execute/CHOP Execute/Parameter Execute callback DATs. Use when a parameter needs to be computed, a table generated, or something to happen on a frame, a channel crossing or a parameter change.
---

# Python in OpenTouchDesigner

Python is embedded through PyO3 (`crates/otd-py/src/lib.rs`). It is **not**
TouchDesigner's `td` module: there is no `op()`, no `me.par`, no `tdu`. Using
those names is an error, not a fallback.

## The scope

Every expression and script sees:

```python
absTime, time, frame, fps    # absolute seconds, timeline seconds, frame, rate
me                           # this operator's path, as a string
math, random, json           # modules
sin cos tan asin acos atan atan2 sqrt floor ceil exp log pi tau
fmod hypot degrees radians   # lifted out of math
clamp(v, lo, hi)
fit(v, oldlo, oldhi, newlo, newhi)
lerp(a, b, t)
smoothstep(lo, hi, v)
```

And four functions into the network:

```python
ch('/audioin1', 'chan1')     # a CHOP channel's current value  -> float
par('/blur1', 'size')        # another operator's parameter
parent('hue')                # a custom parameter on the enclosing Container COMP
setpar('/blur1', 'size', 8)  # write a parameter (scripts and callbacks)
```

The network is reachable **only during evaluation**. Stashing `ch` results in a
module global and reading them later gets you a stale number at best.

## Parameter expressions

Set `mode: Expression` and put the source in `expression`. The built-in
expression language is tried first; anything it cannot parse falls through to
Python automatically, so you write one thing either way.

```ron
"period":    (value: Float(0.25), mode: Expression, expression: "0.25 + sin(absTime) * 0.1"),
"translate": (value: Vec3((0.0,0.0,0.0)), mode: Expression,
              expression: "[0.0, absTime * 12.0, 0.0]"),
"channels":  (value: Str(""), mode: Expression,
              expression: "'band' + str(parent('band'))"),
```

A vector parameter's expression returns a list of the right length.

Operator paths mentioned in an expression become cook-graph dependencies
automatically — you do not wire anything.

For a plain "this channel drives this parameter", prefer `mode: Export` with
`source: "/op:channel"`. It is cheaper and it shows up in the UI as a link.

## Script DAT

`scriptDAT` runs its `source` every cook and takes the table out of a local
named **`rows`** — a list of lists. Cells are stringified individually, so
mixed types are fine.

```python
rows = [['name', 'value']]
for i in range(8):
    rows.append([f'band{i}', ch('/spectrum1', f'band{i + 1}')])
```

No `rows` means an empty table, not an error. In scope here: `absTime`,
`frame`, `me`, plus everything above.

## The callback DATs

Each ships with its signatures as the default body, so the operator documents
itself the moment you drop it.

`executeDAT` — every frame:
```python
def onStart(): pass              # first cook only
def onFrameStart(frame): pass    # before the network cooks
def onFrameEnd(frame): pass      # after
```

`chopexecuteDAT` — a watched channel:
```python
def onValueChange(channel, value, prev): pass
def onOffToOn(channel, value): pass   # crossed the threshold upward
def onOnToOff(channel, value): pass
```
The edge callbacks are the ones worth having — "the beat landed" is an edge,
not a value.

`parameterexecuteDAT` — a watched parameter:
```python
def onValueChange(par, value, prev): pass
```

Execute DATs are always cook roots (a callback that only ran when something
downstream wanted it would be a trap), and `active` gates them. A callback that
raises reports the error on the node and stops nothing else — the frame
continues.

## Errors

Errors are reported as one line on the operator, not a traceback in a console.
Keep expressions short enough that one line is enough to locate the fault; put
anything longer in a Script DAT where you can print.

## Cost

An expression is compiled once and reused. A `chopexecuteDAT` watching a
44.1kHz channel fires per changed sample — watch a `lagCHOP`'d, downsampled
version instead. `otd stats patch.otd --frames 300` shows what a script is
actually costing before you optimise the wrong node.
