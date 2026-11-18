/* The entry record holds pointers (the metadata, `make`): in a shared library they
   are relocated, so the record belongs with the relocated read-only data, and
   `.rodata` stays read-only (the emulator's preflight wants the metadata in a
   segment that is neither writable nor executable). */
SECTIONS
{
  .data.rel.ro.paperpaw : { *(.rodata.paperpaw_entry) }
}
INSERT AFTER .data.rel.ro;
