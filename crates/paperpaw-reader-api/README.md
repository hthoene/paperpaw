# Reader-owned stages, schema version 1 (review draft)

`reader.def` indexes the per-stage canonical JSON schemas under `schemas/`. These tags
are **local to this reader schema**, not interface-7 kernel tags. No record in
this file may be sent to `registrations.submit` or `commands.submit` directly.
Each stage negotiates its schema version through N4. Unknown versions and exact
unknown tags are refused; a numeric tag range never identifies a family.

Every batch is bounded to 4096 bytes and eight records. Insert replies have at
most four items; at most 64 inserted items may be admitted per chapter across
all subscribers. The reader validates aggregate limits, subscriber identity,
source ranges and permissions, not just a structurally valid decoded record.
Limits are proposed for Kernel review, not claims about a landed N4 broker.

| Stage | Input | Reply | Meaning |
|---|---|---|---|
| `reader.insert` | `insert.query` | 0–4 `insert.item` | Before/after source block, stable subscriber-local id, measured height and keep/tap flags. |
| `reader.style` | `style.query`, source `style.block`/`style.run` | `style.block`/`style.run` | Optional fields retain book/reader values when absent. Query text is a bounded UTF-8 chunk. |
| `reader.decor` | `decor.query`, `page.word` | `decor.range` | Source-byte spans with a user-restylable role, item id and tap flag; never change layout metrics. |
| `reader.layer` | `layer.query`, `page.line`/`page.word` | `layer.item` | Side-room reservation and drawing item identity. Layer order is N4 subscriber order. |
| `reader.status` | `status.query` | `status.item` | Bounded text or a custom draw item with measured dimensions. Unknown page counts remain optional. |

Chunked inputs carry a zero-based chunk number and a final-chunk flag (0/1).
Each invocation has its own query record followed by at most seven geometry or
style records. Text positions are UTF-8 byte offsets in the source block;
chunks must end at character boundaries. Geometry uses page coordinates and
pixel units. The reader clips ranges to the supplied source context and rejects
invalid UTF-8 boundaries. No subscriber may retain callback-borrowed strings.

## Pagination and identity

The reader assigns insert ordinals after deterministic N4 subscriber ordering;
subscriber-local item ids are not page-index ordinals. A page-index entry is
`(section, offset, insert)`; zero insert means source text. Progress counts source
text only, while page totals include inserted content. Every layout-changing
subscriber signature and all effective layout settings enter the layout key.
Height must be deterministic for the query and signature. A changed height
requires a new signature and background reindexing anchored to source text.

Kernel-confirmed splitting: a breakable item is cut at one-eighth-page-height
steps (minimum one pixel). Every consecutive piece gets a distinct reader-assigned
insert ordinal; each piece is therefore a valid page boundary. An unbreakable
item moves to the next page if it does not fit the remaining space. Heights over
four page heights are refused and logged. All pieces count zero source progress.
The 64-item chapter admission cap applies after fragmentation as well, so a
single reply cannot bypass the page-index memory budget.

## Styles and user control (decision 131)

Decode validity does not grant authority. The reader applies book styles and
ordered subscriber proposals, then protects each user-selected override unless
the user explicitly allowed that subscriber to override it. A request cannot
self-grant permission. Decorations apply last and affect appearance only.
Metric fields (size, family, weight, spacing, alignment, margins, drop caps)
change the layout key; colour alone does not. Colours are `0x00RRGGBB` with zero
high byte; the X4 host maps sRGB to lightness and its current gray/mono pass.
No new local conversion formula replaces the host's colour contract.

## Drawing, input and images

Draw calls use N2 `draw.frame`; `draw.item` is only its reader-owned payload:
source locator, insert ordinal and subscriber-local item id. Rectangle, clip,
purpose and band ownership come from the outer frame and are never recreated
as an unscoped canvas. Draw inserted items/layers/status only in intersecting
bands. `input.tap` routes only to the owning subscriber's admitted tap target;
normal reader input remains unconsumed elsewhere.

For an image placeholder, obtain dimensions through N6 `image.info` before
layout, then use async `image.open`. On `event.image_ready`, invalidate only the
reserved region through `command.invalidate`. Unknown dimensions use a stable
chosen aspect; the loaded image fits that box. Opt-in relayout changes the
layout signature. The owner releases each image/operation handle exactly once;
cancellation/stop prevents later callbacks and late replies are refused.
This crate defines stage payloads, not a replacement image protocol.

## Boundaries

The generic broker, N2 envelope, font/image handles, permissions, document
provider protocol and host drawing belong to their existing owners. This work
does not start the Reader port or provide a compatibility shim. Wire generation reuses the canonical field/framing templates through
`tools/abi_stage_codec.py`; existing ABI generator outputs remain unchanged.
