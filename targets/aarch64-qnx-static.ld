/* Static AArch64 executable for QNX Neutrino, for GNU ld (aarch64-none-elf).
 *
 * The default aarch64elf script emits ET_EXEC with no PT_PHDR and does not
 * put the ELF headers in a PT_LOAD. procnto then leaves AT_PHDR out of auxv
 * and relibc cannot find PT_TLS.
 *
 * This script keeps a fixed-address ET_EXEC, but:
 *   - maps FILEHDR + PHDRS in the first PT_LOAD (so __ehdr_start exists)
 *   - emits PT_PHDR (so QNX fills AT_PHDR)
 *   - emits PT_TLS and PT_GNU_EH_FRAME
 *   - does not emit PT_INTERP (no ldqnx)
 *   - defines __executable_start and etext, which gcrt1.o profiles between
 *
 * Used as:  aarch64-none-elf-gcc -nostdlib -static -T this_file ...
 */
OUTPUT_FORMAT("elf64-littleaarch64", "elf64-littleaarch64", "elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
ENTRY(_start)

PHDRS
{
    headers  PT_PHDR PHDRS FLAGS(4);                 /* PF_R: table of program headers */
    text     PT_LOAD FILEHDR PHDRS FLAGS(5);         /* PF_R|PF_X */
    data     PT_LOAD FLAGS(6);                       /* PF_R|PF_W */
    tls      PT_TLS FLAGS(4);
    eh_frame 0x6474e550 FLAGS(4);                    /* PT_GNU_EH_FRAME */
}

SECTIONS
{
    PROVIDE (__executable_start = SEGMENT_START("text-segment", 0x400000));
    . = SEGMENT_START("text-segment", 0x400000) + SIZEOF_HEADERS;

    .text : {
        *(.text.unlikely .text.unlikely.*)
        *(.text.exit .text.exit.*)
        *(.text.startup .text.startup.*)
        *(.text.hot .text.hot.*)
        *(SORT(.text.sorted.*))
        *(.text .stub .text.* .gnu.linkonce.t.*)
        *(.gnu.warning)
    } :text
    PROVIDE (__etext = .);
    PROVIDE (_etext = .);
    PROVIDE (etext = .);

    .rodata : {
        *(.rodata .rodata.* .gnu.linkonce.r.*)
    } :text

    .eh_frame_hdr : {
        *(.eh_frame_hdr)
        *(.eh_frame_entry .eh_frame_entry.*)
    } :text :eh_frame

    .eh_frame : ONLY_IF_RO {
        KEEP (*(.eh_frame))
        KEEP (*(.eh_frame.*))
    } :text

    .gcc_except_table : ONLY_IF_RO {
        *(.gcc_except_table .gcc_except_table.*)
    } :text

    .preinit_array : {
        PROVIDE_HIDDEN (__preinit_array_start = .);
        KEEP (*(.preinit_array .preinit_array.*))
        PROVIDE_HIDDEN (__preinit_array_end = .);
    } :text

    .init_array : {
        PROVIDE_HIDDEN (__init_array_start = .);
        KEEP (*(SORT_BY_INIT_PRIORITY(.init_array.*) SORT_BY_INIT_PRIORITY(.ctors.*)))
        KEEP (*(.init_array EXCLUDE_FILE (*crtbegin.o *crtbeginS.o *crtend.o *crtendS.o) .ctors))
        PROVIDE_HIDDEN (__init_array_end = .);
    } :text

    .fini_array : {
        PROVIDE_HIDDEN (__fini_array_start = .);
        KEEP (*(SORT_BY_INIT_PRIORITY(.fini_array.*) SORT_BY_INIT_PRIORITY(.dtors.*)))
        KEEP (*(.fini_array EXCLUDE_FILE (*crtbegin.o *crtbeginS.o *crtend.o *crtendS.o) .dtors))
        PROVIDE_HIDDEN (__fini_array_end = .);
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .tdata : {
        *(.tdata .tdata.* .gnu.linkonce.td.*)
    } :data :tls

    .tbss : {
        *(.tbss .tbss.* .gnu.linkonce.tb.*)
        *(.tcommon)
    } :data :tls

    .data.rel.ro : {
        *(.data.rel.ro.local* .gnu.linkonce.d.rel.ro.local.*)
        *(.data.rel.ro .data.rel.ro.* .gnu.linkonce.d.rel.ro.*)
    } :data

    .got : {
        *(.got)
        *(.igot)
    } :data

    .got.plt : {
        *(.got.plt)
        *(.igot.plt)
    } :data

    .data : {
        *(.data .data.* .gnu.linkonce.d.*)
        SORT(CONSTRUCTORS)
    } :data

    .eh_frame : ONLY_IF_RW {
        KEEP (*(.eh_frame))
        KEEP (*(.eh_frame.*))
    } :data

    .gcc_except_table : ONLY_IF_RW {
        *(.gcc_except_table .gcc_except_table.*)
    } :data

    _edata = .;
    PROVIDE (edata = .);
    __bss_start = .;

    .bss : {
        *(.dynbss)
        *(.bss .bss.* .gnu.linkonce.b.*)
        *(COMMON)
        . = ALIGN(. != 0 ? 8 : 1);
    } :data

    . = ALIGN(8);
    _end = .;
    PROVIDE (end = .);

    /DISCARD/ : {
        *(.interp)
        *(.dynsym)
        *(.dynstr)
        *(.dynamic)
        *(.hash)
        *(.gnu.hash)
        *(.note.GNU-stack)
        *(.gnu_debuglink)
        *(.gnu.lto_*)
    }
}
