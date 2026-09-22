# FFM — requirements not yet scheduled

`TODO.md` holds the phase map, which is complete, and the decisions that were
deliberately deferred. This file holds **new requirements raised after that
map closed** — things nobody has built, designed in enough detail to be built,
and written down so they stay requirements rather than becoming discoveries.

---

## 1. Live notification when the row you are editing changes underneath you

Raised 22/09/2026, while deciding the concurrency question (`TODO.md`, open
item 1). The version check landing now is the **floor**: it guarantees a lost
update is impossible, by refusing the save. This is the ceiling — you find out
*while* you are editing, not when you press Save.

### What it does

Ana saves a change to a row Rodrigo has open in the inline edit form.
Rodrigo's screen, without any action from him:

- A **toast** appears saying the row he is editing has been updated.
- The toast shows the **diff against his current edit** — not against what the
  row said when he opened it, and not Ana's change in isolation. The question
  he actually has is *"does what she did conflict with what I am typing?"*, so
  the comparison is his draft versus the new truth, field by field.
- The form's **Save button becomes Update**. Nothing else about the form
  changes: his typing is untouched, no field is overwritten, no modal steals
  focus. Pressing Update saves his values over the new version.

That is the whole feature. There is no merge UI, no per-field "take mine /
take theirs", no blocking. The household is two to six people; the job is to
stop somebody typing for a minute into a row that has moved, not to build a
three-way merge.

### What it needs

- **A websocket.** Everything else in this app is request/response over HTMX,
  and deliberately so — but this is the one fact the server has that the
  client cannot ask for, because the client does not know to ask. Polling from
  an open edit form would be a request every few seconds for an event that
  happens a handful of times a month.
- **A room per household, a message per committed change.** One process serves
  one household, so the connection is per session and the broadcast carries
  `{entity, entity_id, version, at, member}` — enough for an open form to
  decide whether it cares. The payload deliberately does **not** carry the new
  field values: the client re-reads the row over HTMX when it needs them, so
  privacy filtering keeps happening in exactly one place, in the SELECT.
- **It respects privacy like everything else.** A `private` row's change must
  not announce itself to a member who cannot see the row — which is why the
  broadcast is filtered per subscriber rather than fanned out raw.
- **The toast is a new surface.** This app has no toasts today, on purpose:
  the statusline is the message channel and the unresolved badge is ambient
  precisely because *"a toast gets dismissed and then forgotten"*. This is the
  one case that argues for one, because it is about something happening right
  now to something the person is holding. It should still be written down that
  it is an exception, with the reason, or the next feature will reach for one.
- **It rests on the version column** the concurrency fix introduces. Without
  that, "has this row moved" has no cheap answer; with it, the toast and the
  refused save are the same fact surfaced at two different moments.

### What it does not change

The version check stays exactly as built. If the websocket is down, the
connection dropped, or the browser never opened one, the save is still refused
and the draft is still parked — the guarantee does not depend on the
notification arriving. This is an improvement to *when you find out*, never to
*whether you are protected*.
