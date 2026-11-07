/* How an interface-7 native PaperPaw plugin for the X4 is linked (format 5,
   `abi/ppn5.md`): its read-only data from 0x3C000000, then its code right
   after it in the instruction bus's view of the same memory (0x06000000
   further on), so the two make one image that keeps its shape wherever it
   is loaded.

   Unlike `paperpaw-plugin.x` (interface 6) it has no word for the
   firmware's table and none for the families of shared functions: the host
   table reaches the plugin only as an argument of `make` (`abi/entry.md`).
   The entry and the metadata record come first, the entry 8-aligned and
   the record 4-aligned, as the file's header says where they lie.

   A plugin's project keeps a copy of it at its top, beside its
   `.cargo/config.toml`, whose rustflags link with it
   (`-C link-arg=-Tpaperpaw-plugin7.x`). */

ENTRY(PAPERPAW_ENTRY)

SECTIONS
{
    . = 0x3C000000;
    .rodata : ALIGN(16)
    {
        KEEP(*(.rodata.paperpaw_entry))
        . = ALIGN(4);
        KEEP(*(.rodata.paperpaw_metadata))
        . = ALIGN(4);
        *(.rodata .rodata.* .srodata .srodata.*)
        . = ALIGN(16);
    }

    . = . + 0x06000000;
    .text : ALIGN(16)
    {
        /* All code but the hot code: a section goes to the first pattern
           that names it, so `.text.*` would take the hot code too. */
        *(.literal .text .literal.*
          .text.[!p]* .text.p[!a]* .text.pa[!p]* .text.pap[!e]* .text.pape[!r]*
          .text.paper[!p]* .text.paperp[!a]* .text.paperpa[!w]* .text.paperpaw[!_]*
          .text.paperpaw_[!h]* .text.paperpaw_h[!o]* .text.paperpaw_ho[!t]*
          .text.paperpaw_hot[!.]*)
        /* The hot code (the SDK's `hot!`) at the end, in whole words: a
           device may run it from faster memory (paperpaw-sign pack
           records where it lies). */
        . = ALIGN(4);
        PAPERPAW_HOT = .;
        *(.text.paperpaw_hot .text.paperpaw_hot.*)
        . = ALIGN(4);
        PAPERPAW_HOT_END = .;
    }

    /* Writable data has no place in a plugin (its state lives on the
       kernel's heap): kept here only to be found and refused when the
       file is packed. */
    .data : { *(.data .data.* .sdata .sdata.*) }
    .bss : { *(.bss .bss.* .sbss .sbss.* COMMON) }

    /* The compilers' names stay (`.comment`, not loaded): the file
       keeps which built it, and runs only on firmware built by it. */
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
