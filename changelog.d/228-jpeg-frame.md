### Fixed

- **JPEG decoding no longer needs 3.8 KB of stack** (#228). The quantization and Huffman tables of a JPEG are kept on the heap (1.8 KB on a 32-bit device, freed with the decode, a refused allocation is `OutOfMemory`) instead of in the stack frame of `paperpaw_image::jpeg::decode`, whose frame falls from 3,808 B to 224 B. The X4 plugin packer refuses a plugin that holds a frame of 2,048 B or more. Decoded pixels are unchanged.
