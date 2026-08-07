# The assistant

Describe a patch, get operators.

A floating bar sits over the canvas — type into it and press Enter. It takes
no layout space, and it goes away:

| | |
|---|---|
| `Cmd/Ctrl+K` | show it and put the caret in it, from anywhere |
| `Enter` · `Shift+Enter` | build it · newline |
| `Escape` or `▾` | collapse to a pill; what you typed survives |
| `✕` | hide it entirely (`Cmd/Ctrl+K` brings it back) |
| `✨ Assistant` in the top bar | toggle it |
| `⚙` on the bar | providers, keys, and what was skipped |

It is hidden in perform mode (`F1`), where the window is the show.

It is not a chatbot bolted to the side: the reply is a *plan*, checked against
the real operator registry and built into the network you are looking at, as
one undoable edit.

## Setting it up

Three providers, any one of which is enough:

| Provider | Key from | Default model |
|---|---|---|
| Anthropic | [console.anthropic.com](https://console.anthropic.com/settings/keys) | `claude-sonnet-4-5` |
| OpenAI | [platform.openai.com](https://platform.openai.com/api-keys) | `gpt-5` |
| OpenRouter | [openrouter.ai](https://openrouter.ai/keys) | `anthropic/claude-sonnet-4.5` |

Paste a key into the panel and press **Save**, or set `ANTHROPIC_API_KEY`,
`OPENAI_API_KEY` or `OPENROUTER_API_KEY` in the environment. The environment
wins, because that is how a show machine or a CI job supplies one and neither
should be editing a config file to do it. The model box is a list *and* free
text — the list will be out of date before you read it.

OpenRouter is worth knowing about if you want one key for many models: it
speaks OpenAI's wire format and fronts most of the others.

## Where your key goes

**Never into a project file.** `.otd` is text, it is meant to be committed,
and the whole pitch of the format is that you can read it in a diff. A key in
there is a key in somebody's git history, and a key in a git history has to be
*rotated*, not deleted.

Keys go in one file outside every project, `0600`:

| | |
|---|---|
| macOS | `~/Library/Application Support/OpenTouchDesigner/keys.conf` |
| Linux | `~/.config/opentouchdesigner/keys.conf` |
| Windows | `%APPDATA%\OpenTouchDesigner\keys.conf` |

Nothing prints one. The key type has a hand-written `Debug` that redacts, so a
stray `{:?}` in a log cannot leak it, and provider errors are scrubbed before
they reach the panel — providers quote the offending header back often enough
that this matters.

## How it knows what to build with

The model is told about the operators **by the registry** — the same table the
editor builds its menus and `OPERATORS.md` from. So an operator added tomorrow
is one the assistant can use tomorrow, and one that was renamed cannot be
suggested under its old name. There is no second list to keep in sync, which
is the same trick the operator reference already uses, pointed at a different
reader.

It also gets a description of the network you are in, so "add trails to this"
means the thing on your canvas.

## What stops it breaking your patch

The reply is JSON, and it is validated before anything is created:

- **An operator that does not exist fails the whole plan.** Not "apply the
  parts that work" — half a patch is worse than none, because then you have to
  work out which half.
- **A parameter that does not exist is dropped**, and listed under *things
  skipped*. Models invent plausible parameter names; that is not a reason to
  refuse the other eight nodes.
- **Values are coerced to the parameter's declared type.** `1` for a float
  parameter becomes `1.0`; `"scale": 1.03` on a two-component parameter
  broadcasts; a colour given as three numbers is opaque.
- **A name collision renames** rather than overwriting, and the plan's wires
  follow the renamed nodes rather than the ones already there.
- **It is one undo.** A checkpoint is taken before the first node is created.
  `Cmd/Ctrl+Z` removes the whole thing.

**Shaders are compiled before the node exists.** If the model reaches for a
GLSL TOP, the source goes through naga — the same front end that would reject
it a moment later — *before* anything is created. A shader that fails is sent
back to the model with the compiler's own error, once. That error is the most
useful thing anybody has; showing you a red node instead of using it is a
waste of it. Once and not until it works: a model that cannot fix its own
shader on the second go will not on the fifth, and you are waiting.

Anything still broken after that is reported as a warning rather than left as
a red node and a silently black output. So is a chain the model built and
forgot to wire in — which is what they do when asked to *add* to a patch.

The network is only ever added to. Nothing is deleted or rewired behind you.

## Shaders: GLSL, not WGSL

The assistant is told to write Shadertoy-style GLSL — `void mainImage(out
vec4 fragColor, in vec2 fragCoord)` with `iTime` and `iResolution` — and to
set the node's `language` to `glsl`, even though WGSL is this tool's primary
dialect.

That is deliberate and it is the single biggest thing that made shaders work.
Asked for WGSL, `gpt-5-mini` produced a shader that would not compile, and
*still* would not after being handed the compiler error. Asked for GLSL, it
compiled first time. GLSL is the dialect with a million worked examples in
every model's training data; WGSL is not, yet. Meeting the model where it is
beats insisting on the house style, and the GLSL TOP accepts both anyway.

## Prompts that work

Concrete and visual beats clever:

- *A slow feedback tunnel in deep blue and gold*
- *Concentric rings pulsing outward from the centre*
- *A kaleidoscope over drifting noise, with trails*
- *Something that reacts to the microphone*

The system prompt already carries the taste from [GUIDE.md](GUIDE.md) — that
motion comes from a loop, that a loop needs contrast rather than a grey wash,
that colour is better done by multiplying a monochrome source through a ramp.
You do not have to ask for those; you do have to ask for a *look*.

Expect to steer it. What comes back is a starting patch with the wiring done
and the numbers approximately right, which is the boring half. Turning
`decay.brightness` and `zoom.scale` until it looks like something is the half
you wanted anyway.

## Without the editor

```bash
cargo run -p otd-ai --example smoke        # call every provider that has a key
cargo run -p otd-ai --example build -- openrouter "a slow blue tunnel" out.otd
otd render out.otd --node /out1 --frames 150 --out shots
```

`build` writes an ordinary `.otd`. That it then renders is the actual test of
whether any of this works — a plan that parses but does not draw is not a
patch.
