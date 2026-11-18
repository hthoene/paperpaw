### Changed

- **A native plugin may register up to 32 callbacks** (#228). The cap on event subscriptions plus draw callbacks, counted over the whole construction, grows from 8 to 32; beyond it the batch is still refused with `ERR_NO_SPACE`. The records are kept in a list grown per batch (`ERR_NO_MEMORY` if it cannot grow, nothing staged), so a plugin costs only the heap of what it registered instead of eight fixed slots.
