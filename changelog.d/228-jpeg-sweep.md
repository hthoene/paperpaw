### Fixed

- **A JPEG's component list, a progressive scan's member list and the default colour and see-through row conversions no longer abort on a full heap** (#228). Decoding a baseline or progressive JPEG, or handing a sink colour or see-through rows it leaves to the default, on a heap with no room now ends with an out-of-memory refusal, as the rest of the decoders do. A sweep test refuses each allocation of real JPEGs and PNGs in turn.
