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

## Working back from a picture

Attach a still and the assistant builds a patch that produces that look.
Click **📎** in the bar, or drop an image straight onto the bar — dropped
anywhere else it is still material, and becomes a Movie File In, which is what
dropping a picture on a node graph should do. The overlay says which one is
about to happen while the file is still in the air.

With a reference attached the prompt box becomes optional: pointing at a
picture is a complete request. Anything you do type is a correction on top of
it — *"like this but slower"*, *"these colours, no feedback"* — and wins where
the two disagree.

What comes back is the **recipe**, not the frame. The brief that goes with the
image asks for structure before colour, for a feedback loop wherever the image
smears or echoes, and for the interesting quantities to stay on separate nodes
as parameters. The failure mode this is aimed at is one enormous glslTOP that
hard-codes what it sees: a screenshot with extra steps, which cannot be turned
and teaches you nothing about your own patch. Read the `notes` first — it says
what it thought the image was made of, which is worth knowing even when the
patch needs work.

All five providers take an image. Anthropic and OpenAI take it inline, Codex
takes a file, and Claude Code takes it as stream-json — none of which you have
to care about, except that it is one more reason the CLI providers cost quota.

Everything is shrunk to 1568 pixels on the long edge before it is sent, and
re-encoded — JPEG normally, PNG when the image genuinely uses transparency,
because flattening an alpha channel changes the question being asked. Past that
size every provider resizes it themselves, having already charged for it.

## Setting it up

Five providers, any one of which is enough. Three want an API key:

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

## Using a subscription instead of a key

The other two want nothing at all, because you are already paying for them:

| Provider | Needs | Costs |
|---|---|---|
| Claude Code | the `claude` CLI, signed in | Claude Pro/Max quota |
| Codex | the `codex` CLI, signed in | ChatGPT Plus/Pro quota |

Pick one in the panel and it says which version it found instead of showing a
key field. There is no key to paste and nothing is stored.

These do not call an API. They run the CLI you already signed in with — `claude
-p`, `codex exec` — as a subprocess, put the prompt on its stdin and read the
reply off its stdout. That is the difference between using a subscription and
misusing one: no token is lifted out of anybody's credential store, and no
request is made to an API endpoint on a subscription's behalf.

What this costs you instead of money:

- **A second or two more per ask.** Process startup plus the CLI's own preamble
  before any token moves.
- **Quota, not credit.** Claude Code sends its tool schemas whether or not the
  tools are allowed, so a patch costs around 28k input tokens against the same
  weekly limit your editor session draws on.
- **A machine, not a server.** It works where the CLI is installed and signed
  in. For a show machine or CI, use a key.

Both are run with the agent turned off — tools denied, sandbox read-only,
`CLAUDE.md`/skills/plugins/MCP off, working directory a temp dir, no session
written. It is asked for one JSON object and given nothing it could do anything
else with.

Neither CLI is usually on `PATH` for an application launched from Finder or the
Start menu, so the usual install locations are searched too. If yours is
somewhere unusual, set `OTD_CLAUDE_BIN` or `OTD_CODEX_BIN` to the binary.

Codex's model box defaults to **(CLI default)** — an empty model, meaning
whatever `~/.codex/config.toml` says. This is deliberate: Codex refuses outright
on a model the signed-in account has no access to, and which those are depends
on the plan, so there is no safe guess to make from here. Claude Code takes the
aliases `sonnet`, `opus` and `haiku`, which always resolve to what is current.

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

## Changing and removing, not just adding

For a long time the network was only ever added to, and that was the wrong
promise. "Make it simpler" is one of the most common things anybody types, and
a model that can only add answers it by adding — which is the opposite. So a
plan now carries three verbs:

- `nodes` creates, as it always did.
- `set` retunes an operator that already exists. It exists because naming one
  under `nodes` gets you a *renamed duplicate* beside the original with the
  original untouched, which is a confusing way to fail at "slow the zoom down".
- `delete` removes operators, by name.

Deleting out of the middle of a chain splices the node's input into whatever
it fed, so the chain stays joined. That is not a nicety: without it, "make
this simpler" leaves the survivors with an empty input 0, which is a black
texture and looks like the delete broke the patch.

Deletion is fenced in three ways, none of which cost anything when the model
has read the request correctly:

- **Direct children of the network on screen only.** A name can resolve to a
  path inside a component you are not looking at. A delete you cannot watch
  happen is not one you can catch.
- **Never a node the same plan just created.** That is a model talking to
  itself, and honouring it makes the reported node count a lie.
- **Never the whole network.** Clearing the canvas is `/clear` — one word,
  undoable, obviously deliberate. Arriving there through "make it simpler" is
  not what anybody meant.

It is still one undo. The checkpoint is taken before the first change of any
kind, so `Cmd/Ctrl+Z` puts back everything a plan created, retuned *and*
removed, in one go.

## Operators named by parameter

Not everything in a patch is joined by a wire. A Render TOP has no inputs at
all — it renders the Geometry, Camera and Light COMPs named in its
parameters. A Geometry COMP names the SOP it draws, the MAT that shades it
and the CHOP that instances it. Select TOP names any TOP anywhere.

The model writes those as plain names, before any node has a path and before
the graph has renamed anything that collided, so `apply` rewrites them.
It used to rewrite exactly one: a Feedback TOP's `target`. Everything else
kept the bare name and quietly pointed at nothing — which is the whole 3D and
instancing half of the program building perfectly and rendering an empty
frame. It now rewrites every parameter the registry marks `is_path_ref`, so
operators added later are covered without a list to maintain.

The scenario harness prints `DANGLING REF` for any path parameter that does
not resolve after a plan is applied, because no warning covers it and the
symptom is a black texture.

## Reacting to something

`expressions` animates on a clock, and a clock is not a reaction — the patch
does the same thing whether anybody is in the room or not. Reacting is
**Export**: a parameter reads a live channel off a CHOP, the same edit as
dragging a channel onto a parameter row in the editor.

The plan format had no way to say that, which meant the assistant could not
build a reactive patch at all. Asked to make a camera patch respond to
somebody moving, it built a feedback loop tuned by `absTime` — busy, and
completely indifferent to the room. So a node now carries `exports`
alongside `params` and `expressions`:

```json
{"name": "zoom1", "op": "transformTOP", "exports": {"scale": "lag1:r"}}
```

The value is `node:channel`, the node resolved like any other name, so a plan
can create the whole CHOP chain and export off it in one answer. Exporting
from a TOP is refused rather than accepted: a parameter pointed at something
with no channels does not error at cook time, it just never moves, which
looks exactly like a patch that does not react and gives nobody anywhere to
look.

The brief also teaches the chain, because it is not guessable — measuring
*movement* means measuring change, not brightness:

    camera -> compositeTOP "difference" <- feedbackTOP targeting it
           -> toptochopCHOP "average" -> analyzeCHOP -> lagCHOP -> exports

## Commands the box answers itself

The prompt box is the only place in the editor you type a sentence, so it is
where a slash command gets typed whether or not one exists. Two do, and
neither costs a round trip:

- `/clear` empties the network you are looking at. One undo.
- `/help` lists them.

Anything else starting with `/` is reported as an unknown command rather than
sent to a model, which would otherwise answer "/clear" with a patch about
clearing — slowly, and for money.

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

Meeting the model where it is has a cost, though, and it took a black screen
to find it. Every model that knows what a node-based visual tool is knows
TouchDesigner, so asked for "crazy effects on top of my video" it wrote
`texture(sTD2DInputs[0], uv)` — the right idea in the wrong dialect. That name
does not exist here, the shader did not compile, the GLSL TOP output black, and
eleven operators downstream of it dutifully passed the black along. The patch
was wired correctly end to end and the viewer was dark.

So the brief now names the input sampler — `iChannel0` and `iChannel1`, as
Shadertoy has them — says which TouchDesigner spellings are *not* names here,
and the repair round trip repeats it, because "Unknown variable: sTD2DInputs"
is not an error a model can act on without being told what the variable should
have been. `crates/otd-ai/tests/shader_brief.rs` compiles every name the brief
promises and every name it warns off, so the prose and the compiler cannot
drift apart quietly a second time.

A shader that still will not compile is now said out loud in the assistant bar
rather than folded into the collapsed "things skipped" list. It is the one
failure that looks like a success: the node count is right, the notes read
well, and the picture is black.

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
cargo run -p otd-ai --example build -- claude-code "a slow blue tunnel" out.otd
otd render out.otd --node /out1 --frames 150 --out shots
```

`build` writes an ordinary `.otd`. That it then renders is the actual test of
whether any of this works — a plan that parses but does not draw is not a
patch.
