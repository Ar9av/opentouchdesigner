# The assistant

Describe a patch, get operators. **✨ Assistant** in the top bar.

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

The network is only ever added to. Nothing is deleted or rewired behind you.

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
